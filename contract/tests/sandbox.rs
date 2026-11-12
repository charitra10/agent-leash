//! Integration tests: the real compiled contract on a local NEAR sandbox,
//! with real access keys. These prove what unit tests can't: that the NEAR
//! protocol itself stops a restricted key, end to end.
//!
//! Build the wasm first:  cargo near build non-reproducible-wasm
//! Then run:              cargo test --test sandbox

use near_workspaces::network::Sandbox;
use near_workspaces::result::ExecutionFinalResult;
use near_workspaces::types::{AccessKey, Gas, KeyType, NearToken, SecretKey};
use near_workspaces::{Account, Contract, Worker};
use serde_json::{json, Value};

const WASM: &str = "target/near/agent_leash.wasm";

fn near(n: u128) -> NearToken {
    NearToken::from_near(n)
}
fn milli(n: u128) -> NearToken {
    NearToken::from_millinear(n)
}
fn yocto(t: NearToken) -> String {
    t.as_yoctonear().to_string()
}
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);

struct Env {
    worker: Worker<Sandbox>,
    owner: Account,
    /// The agent signing with ONLY its function-call key (like the real agent process).
    agent: Account,
    api: Account,
    scammer: Account,
    guardian: Account,
    leash: Contract,
}

/// Add a function-call key to `account` and return an Account that signs with it.
async fn restricted_key(
    worker: &Worker<Sandbox>,
    account: &Account,
    receiver: &Contract,
    methods: &[&str],
) -> anyhow::Result<Account> {
    let sk = SecretKey::from_random(KeyType::ED25519);
    account
        .batch(account.id())
        .add_key(
            sk.public_key(),
            AccessKey::function_call_access(receiver.id(), methods, Some(milli(250))),
        )
        .transact()
        .await?
        .into_result()?;
    Ok(Account::from_secret_key(account.id().clone(), sk, worker))
}

async fn setup(tripwire: Option<Value>) -> anyhow::Result<Env> {
    let wasm = std::fs::read(WASM)
        .expect("target/near/agent_leash.wasm not found: run `cargo near build non-reproducible-wasm` first");
    let worker = near_workspaces::sandbox().await?;
    let owner = worker.dev_create_account().await?;

    let sub = |name: &'static str, bal: NearToken| {
        let owner = owner.clone();
        async move {
            anyhow::Ok(
                owner
                    .create_subaccount(name)
                    .initial_balance(bal)
                    .transact()
                    .await?
                    .into_result()?,
            )
        }
    };
    let leash_acc = sub("leash", near(20)).await?;
    let agent_full = sub("agent", near(2)).await?;
    let api = sub("api", near(1)).await?;
    let scammer = sub("scammer", near(1)).await?;
    let guardian = sub("guardian", near(2)).await?;

    let leash = leash_acc.deploy(&wasm).await?.into_result()?;
    leash
        .call("new")
        .args_json(json!({
            "owner_id": owner.id(),
            "agent_id": agent_full.id(),
            "policy": {
                "max_per_tx": yocto(near(2)),
                "auto_approve_limit": yocto(milli(500)),
                "window_limit": yocto(near(3)),
                "window_sec": 86400,
                "allowlist_only": true,
            },
            "allowlist": [api.id()],
            "guardians": [guardian.id()],
            "tripwire": tripwire,
        }))
        .transact()
        .await?
        .into_result()?;

    let agent = restricted_key(&worker, &agent_full, &leash, &["pay"]).await?;
    Ok(Env { worker, owner, agent, api, scammer, guardian, leash })
}

async fn pay(env: &Env, to: &Account, amount: NearToken, memo: &str) -> anyhow::Result<Value> {
    Ok(env
        .agent
        .call(env.leash.id(), "pay")
        .args_json(json!({ "receiver_id": to.id(), "amount": yocto(amount), "memo": memo }))
        .gas(Gas::from_tgas(30))
        .transact()
        .await?
        .into_result()?
        .json()?)
}

async fn status(env: &Env) -> anyhow::Result<Value> {
    Ok(env.leash.view("get_status").await?.json()?)
}

async fn balance(a: &Account) -> anyhow::Result<NearToken> {
    Ok(a.view_account().await?.balance)
}

/// The transaction must fail (either rejected before execution, or failed during it),
/// and the error must mention `needle`.
fn assert_rejected(res: anyhow::Result<ExecutionFinalResult>, needle: &str) {
    let text = match res {
        Err(e) => format!("{e:?}"),
        Ok(r) => {
            assert!(r.is_failure(), "expected failure mentioning {needle:?}, but it succeeded");
            format!("{:?}", r.into_result().unwrap_err())
        }
    };
    assert!(text.contains(needle), "expected error mentioning {needle:?}, got: {text}");
}

fn ok<T>(r: Result<T, near_workspaces::error::Error>) -> anyhow::Result<T> {
    Ok(r?)
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_key_can_only_call_pay() -> anyhow::Result<()> {
    let env = setup(None).await?;

    // Allowed: pay through the leash.
    let a = pay(&env, &env.api, milli(100), "API credits").await?;
    assert_eq!(a["outcome"], "Sent");

    // Rejected by the protocol: sending NEAR directly.
    let res = ok(env.agent.transfer_near(env.scammer.id(), milli(100)).await);
    assert_rejected(res, "RequiresFullAccess");

    // Rejected by the protocol: calling a method the key doesn't allow.
    let res = ok(env
        .agent
        .call(env.leash.id(), "set_policy")
        .args_json(json!({ "policy": {} }))
        .transact()
        .await);
    assert_rejected(res, "MethodNameMismatch");

    // Rejected by the protocol: the key only works on the leash contract.
    let res = ok(env.agent.call(env.api.id(), "pay").transact().await);
    assert_rejected(res, "ReceiverMismatch");
    Ok(())
}

#[tokio::test]
async fn attack_story_moves_exactly_the_allowed_money() -> anyhow::Result<()> {
    let env = setup(None).await?;
    let api_before = balance(&env.api).await?;
    let scam_before = balance(&env.scammer).await?;

    assert_eq!(pay(&env, &env.api, milli(500), "ok").await?["outcome"], "Sent");
    let scam = pay(&env, &env.scammer, milli(100), "refund fee").await?;
    assert_eq!(scam["reason"], "recipient_not_allowlisted");
    let drain = pay(&env, &env.api, near(10), "drain").await?;
    assert_eq!(drain["reason"], "over_per_tx_limit");
    let pending = pay(&env, &env.api, near(1), "GPU").await?;
    assert_eq!(pending["outcome"], "Pending");

    // Slow drain: 0.5 already spent, window is 3 -> five more 0.5 payments fit.
    let mut sent = 0;
    for _ in 0..8 {
        if pay(&env, &env.api, milli(500), "micro").await?["outcome"] == "Sent" {
            sent += 1;
        }
    }
    assert_eq!(sent, 5);

    // Owner approves the pending 1 NEAR (owner decision may exceed the window).
    let approved: Value = env
        .owner
        .call(env.leash.id(), "approve")
        .args_json(json!({ "id": pending["id"] }))
        .deposit(ONE_YOCTO)
        .gas(Gas::from_tgas(30))
        .transact()
        .await?
        .into_result()?
        .json()?;
    assert_eq!(approved["outcome"], "Approved");

    // Exactly 0.5 + 5 x 0.5 + 1 = 4 NEAR reached the API; the scammer got nothing.
    assert_eq!(
        balance(&env.api).await?.as_yoctonear() - api_before.as_yoctonear(),
        near(4).as_yoctonear()
    );
    assert_eq!(balance(&env.scammer).await?, scam_before);

    let s = status(&env).await?;
    assert_eq!(s["total_sent"], 7);
    assert_eq!(s["total_blocked"], 5);
    Ok(())
}

#[tokio::test]
async fn tripwire_freezes_the_agent_on_chain() -> anyhow::Result<()> {
    let env = setup(Some(json!({ "max_blocked": 3, "period_sec": 3600 }))).await?;

    pay(&env, &env.scammer, milli(100), "1").await?;
    pay(&env, &env.api, near(10), "2").await?;
    assert_eq!(status(&env).await?["frozen"], false);

    // The third strike: the event log shows the auto-freeze.
    let res = env
        .agent
        .call(env.leash.id(), "pay")
        .args_json(json!({ "receiver_id": env.scammer.id(), "amount": yocto(milli(1)), "memo": "3" }))
        .gas(Gas::from_tgas(30))
        .transact()
        .await?
        .into_result()?;
    assert!(res.logs().iter().any(|l| l.contains("\"auto_frozen\"")));

    let s = status(&env).await?;
    assert_eq!(s["frozen"], true);
    assert_eq!(s["frozen_by"], "tripwire");
    assert_eq!(pay(&env, &env.api, milli(10), "valid").await?["reason"], "frozen");
    Ok(())
}

#[tokio::test]
async fn guardian_bot_key_can_freeze_but_never_unfreeze() -> anyhow::Result<()> {
    let env = setup(None).await?;
    // A monitoring bot gets a cheap key that may only call `freeze`.
    let bot = restricted_key(&env.worker, &env.guardian, &env.leash, &["freeze"]).await?;

    bot.call(env.leash.id(), "freeze").transact().await?.into_result()?;
    let s = status(&env).await?;
    assert_eq!(s["frozen"], true);
    assert_eq!(s["frozen_by"], env.guardian.id().to_string());

    // The bot key can't even attempt to unfreeze...
    let res = ok(bot.call(env.leash.id(), "unfreeze").transact().await);
    assert_rejected(res, "MethodNameMismatch");
    // ...and the guardian's full-access key is refused by the contract.
    let res = ok(env
        .guardian
        .call(env.leash.id(), "unfreeze")
        .deposit(ONE_YOCTO)
        .transact()
        .await);
    assert_rejected(res, "Only the owner can do this");

    // Only the owner can release it.
    env.owner
        .call(env.leash.id(), "unfreeze")
        .deposit(ONE_YOCTO)
        .transact()
        .await?
        .into_result()?;
    assert_eq!(status(&env).await?["frozen"], false);
    Ok(())
}

#[tokio::test]
async fn owner_powers_need_a_full_access_key() -> anyhow::Result<()> {
    let env = setup(None).await?;
    // Even a key OF THE OWNER, if it is a limited function-call key (say, one a
    // dapp holds), cannot loosen the leash:
    let dapp_key = restricted_key(&env.worker, &env.owner, &env.leash, &["set_policy"]).await?;
    let loose = json!({ "policy": {
        "max_per_tx": yocto(near(100)), "auto_approve_limit": yocto(near(100)),
        "window_limit": yocto(near(100)), "window_sec": 86400, "allowlist_only": false } });

    // Function-call keys may not attach a deposit...
    let res = ok(dapp_key
        .call(env.leash.id(), "set_policy")
        .args_json(&loose)
        .deposit(ONE_YOCTO)
        .transact()
        .await);
    assert_rejected(res, "DepositWithFunctionCall");
    // ...and without the 1 yoctoNEAR the contract refuses.
    let res = ok(dapp_key.call(env.leash.id(), "set_policy").args_json(&loose).transact().await);
    assert_rejected(res, "Requires attached deposit of exactly 1 yoctoNEAR");

    // The owner's full-access key works.
    env.owner
        .call(env.leash.id(), "set_policy")
        .args_json(&loose)
        .deposit(ONE_YOCTO)
        .transact()
        .await?
        .into_result()?;
    assert_eq!(status(&env).await?["policy"]["allowlist_only"], false);
    Ok(())
}
