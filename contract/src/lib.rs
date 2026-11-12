//! Agent Leash: a spending-policy wallet for AI agents.
//!
//! The contract holds the funds. The agent may only call `pay`; every rule is
//! enforced here, on-chain. Blocked attempts are recorded (not panicked) so the
//! owner can see what the agent tried to do.

use near_sdk::serde_json::{self, json};
use near_sdk::store::IterableSet;
use near_sdk::{
    assert_one_yocto, env, near, require, AccountId, BorshStorageKey, NearToken, PanicOnDefault,
    Promise,
};

/// How many past attempts are kept on-chain (oldest dropped first).
pub const MAX_HISTORY: usize = 50;
/// How many payments may wait for owner approval at once.
pub const MAX_PENDING: usize = 20;
/// Memos longer than this are truncated.
pub const MAX_MEMO_LEN: usize = 80;
/// Balance always kept back so the contract can pay for its own storage.
pub const BALANCE_RESERVE: NearToken = NearToken::from_millinear(100); // 0.1 NEAR

#[near]
#[derive(BorshStorageKey)]
enum StorageKey {
    Allowlist,
    Blocklist,
    Guardians,
}

/// Auto-freeze: if the agent racks up `max_blocked` blocked attempts within
/// `period_sec`, the contract freezes it on the spot, no human needed.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Tripwire {
    pub max_blocked: u32,
    pub period_sec: u64,
}

fn validate_tripwire(t: &Tripwire) {
    require!(t.max_blocked > 0, "max_blocked must be > 0");
    require!(t.period_sec > 0, "period_sec must be > 0");
}

/// The rules the agent must follow. All amounts are in yoctoNEAR (JSON strings).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    /// Payments above this are always blocked.
    pub max_per_tx: NearToken,
    /// Payments above this (but within `max_per_tx`) wait for owner approval.
    pub auto_approve_limit: NearToken,
    /// Total the agent may spend per rolling window.
    pub window_limit: NearToken,
    /// Length of the spending window in seconds (e.g. 86400 = one day).
    pub window_sec: u64,
    /// If true, the agent may only pay accounts on the allowlist.
    pub allowlist_only: bool,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Sent,
    Pending,
    Blocked,
    Approved,
    Rejected,
}

/// One payment request from the agent and what happened to it.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Attempt {
    pub id: u64,
    pub receiver_id: AccountId,
    pub amount: NearToken,
    pub memo: String,
    pub timestamp_ms: u64,
    pub outcome: Outcome,
    /// Why it was blocked (machine-readable), e.g. "over_window_limit".
    pub reason: Option<String>,
}

#[near(serializers = [json])]
pub struct Status {
    pub owner_id: AccountId,
    pub agent_id: AccountId,
    pub frozen: bool,
    pub policy: Policy,
    pub spent_in_window: NearToken,
    pub window_resets_at_ms: u64,
    pub available_balance: NearToken,
    pub total_sent: u64,
    pub total_blocked: u64,
    pub pending_count: u32,
    /// Who froze it: the owner's or a guardian's account id, or "tripwire".
    pub frozen_by: Option<String>,
    pub tripwire: Option<Tripwire>,
    /// Blocked attempts that currently count towards the tripwire.
    pub strikes: u32,
    pub guardian_count: u32,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct AgentLeash {
    owner_id: AccountId,
    agent_id: AccountId,
    policy: Policy,
    frozen: bool,
    allowlist: IterableSet<AccountId>,
    blocklist: IterableSet<AccountId>,
    window_start_ms: u64,
    spent_in_window: NearToken,
    next_id: u64,
    history: Vec<Attempt>,
    pending: Vec<Attempt>,
    total_sent: u64,
    total_blocked: u64,
    // ---- added in v2 ----
    guardians: IterableSet<AccountId>,
    tripwire: Option<Tripwire>,
    /// Timestamps (ms) of recent blocked attempts, newest last.
    recent_blocks_ms: Vec<u64>,
    frozen_by: Option<String>,
}

/// State layout of v1 (before guardians and tripwire), used by `migrate`.
#[near(serializers = [borsh])]
pub struct AgentLeashV1 {
    owner_id: AccountId,
    agent_id: AccountId,
    policy: Policy,
    frozen: bool,
    allowlist: IterableSet<AccountId>,
    blocklist: IterableSet<AccountId>,
    window_start_ms: u64,
    spent_in_window: NearToken,
    next_id: u64,
    history: Vec<Attempt>,
    pending: Vec<Attempt>,
    total_sent: u64,
    total_blocked: u64,
}

fn emit(event: &str, data: serde_json::Value) {
    let ev = json!({
        "standard": "agent_leash",
        "version": "1.0.0",
        "event": event,
        "data": [data],
    });
    env::log_str(&format!("EVENT_JSON:{}", ev));
}

fn validate_policy(p: &Policy) {
    require!(p.window_sec > 0, "window_sec must be > 0");
    require!(
        p.auto_approve_limit <= p.max_per_tx,
        "auto_approve_limit must be <= max_per_tx"
    );
}

#[near]
impl AgentLeash {
    #[init]
    pub fn new(
        owner_id: AccountId,
        agent_id: AccountId,
        policy: Policy,
        allowlist: Option<Vec<AccountId>>,
        guardians: Option<Vec<AccountId>>,
        tripwire: Option<Tripwire>,
    ) -> Self {
        validate_policy(&policy);
        if let Some(t) = &tripwire {
            validate_tripwire(t);
        }
        let mut this = Self {
            owner_id,
            agent_id,
            policy,
            frozen: false,
            allowlist: IterableSet::new(StorageKey::Allowlist),
            blocklist: IterableSet::new(StorageKey::Blocklist),
            window_start_ms: env::block_timestamp_ms(),
            spent_in_window: NearToken::from_yoctonear(0),
            next_id: 0,
            history: Vec::new(),
            pending: Vec::new(),
            total_sent: 0,
            total_blocked: 0,
            guardians: IterableSet::new(StorageKey::Guardians),
            tripwire,
            recent_blocks_ms: Vec::new(),
            frozen_by: None,
        };
        for a in allowlist.unwrap_or_default() {
            this.allowlist.insert(a);
        }
        for g in guardians.unwrap_or_default() {
            this.guardians.insert(g);
        }
        this
    }

    /// One-time upgrade from the v1 state layout. Keeps all history and counters.
    #[private]
    #[init(ignore_state)]
    pub fn migrate() -> Self {
        let old: AgentLeashV1 =
            env::state_read().unwrap_or_else(|| env::panic_str("No v1 state to migrate"));
        Self {
            frozen_by: old.frozen.then(|| old.owner_id.to_string()),
            owner_id: old.owner_id,
            agent_id: old.agent_id,
            policy: old.policy,
            frozen: old.frozen,
            allowlist: old.allowlist,
            blocklist: old.blocklist,
            window_start_ms: old.window_start_ms,
            spent_in_window: old.spent_in_window,
            next_id: old.next_id,
            history: old.history,
            pending: old.pending,
            total_sent: old.total_sent,
            total_blocked: old.total_blocked,
            guardians: IterableSet::new(StorageKey::Guardians),
            tripwire: None,
            recent_blocks_ms: Vec::new(),
        }
    }

    // ------------------------------------------------------------------
    // Agent
    // ------------------------------------------------------------------

    /// Called by the agent to pay someone. Never panics on a policy violation:
    /// the attempt is recorded as Blocked with a reason instead.
    pub fn pay(&mut self, receiver_id: AccountId, amount: NearToken, memo: Option<String>) -> Attempt {
        require!(
            env::predecessor_account_id() == self.agent_id,
            "Only the agent can call pay"
        );
        self.roll_window();

        let mut memo = memo.unwrap_or_default();
        if memo.len() > MAX_MEMO_LEN {
            let mut cut = MAX_MEMO_LEN;
            while !memo.is_char_boundary(cut) {
                cut -= 1;
            }
            memo.truncate(cut);
        }

        let mut attempt = Attempt {
            id: self.next_id,
            receiver_id,
            amount,
            memo,
            timestamp_ms: env::block_timestamp_ms(),
            outcome: Outcome::Blocked,
            reason: None,
        };
        self.next_id += 1;

        match self.check(&attempt) {
            Err(reason) => {
                attempt.reason = Some(reason.to_string());
                self.total_blocked += 1;
                self.record(&attempt, "payment_blocked");
                // Attempts while already frozen don't count as new strikes.
                if reason != "frozen" {
                    self.strike();
                }
            }
            Ok(true) => {
                // Needs owner approval.
                attempt.outcome = Outcome::Pending;
                self.pending.push(attempt.clone());
                self.record(&attempt, "payment_pending");
            }
            Ok(false) => {
                attempt.outcome = Outcome::Sent;
                self.spent_in_window = self.spent_in_window.saturating_add(attempt.amount);
                self.total_sent += 1;
                Promise::new(attempt.receiver_id.clone()).transfer(attempt.amount).detach();
                self.record(&attempt, "payment_sent");
            }
        }
        attempt
    }

    // ------------------------------------------------------------------
    // Owner (all require 1 yoctoNEAR => full-access key signature)
    // ------------------------------------------------------------------

    #[payable]
    pub fn set_policy(&mut self, policy: Policy) {
        self.assert_owner();
        validate_policy(&policy);
        self.policy = policy.clone();
        emit("policy_updated", serde_json::to_value(&policy).unwrap());
    }

    #[payable]
    pub fn set_agent(&mut self, agent_id: AccountId) {
        self.assert_owner();
        self.agent_id = agent_id.clone();
        emit("agent_changed", json!({ "agent_id": agent_id }));
    }

    #[payable]
    pub fn allow(&mut self, account_ids: Vec<AccountId>) {
        self.assert_owner();
        for a in &account_ids {
            self.allowlist.insert(a.clone());
        }
        emit("allowlist_added", json!({ "account_ids": account_ids }));
    }

    #[payable]
    pub fn disallow(&mut self, account_ids: Vec<AccountId>) {
        self.assert_owner();
        for a in &account_ids {
            self.allowlist.remove(a);
        }
        emit("allowlist_removed", json!({ "account_ids": account_ids }));
    }

    #[payable]
    pub fn block(&mut self, account_ids: Vec<AccountId>) {
        self.assert_owner();
        for a in &account_ids {
            self.blocklist.insert(a.clone());
        }
        emit("blocklist_added", json!({ "account_ids": account_ids }));
    }

    #[payable]
    pub fn unblock(&mut self, account_ids: Vec<AccountId>) {
        self.assert_owner();
        for a in &account_ids {
            self.blocklist.remove(a);
        }
        emit("blocklist_removed", json!({ "account_ids": account_ids }));
    }

    /// Pull the emergency brake. The owner or any guardian can do this, and it
    /// needs no deposit: freezing only ever makes things safer, so a guardian
    /// bot can hold a cheap function-call key that is limited to `freeze`.
    pub fn freeze(&mut self) {
        let caller = env::predecessor_account_id();
        require!(
            caller == self.owner_id || self.guardians.contains(&caller),
            "Only the owner or a guardian can freeze"
        );
        if !self.frozen {
            self.frozen = true;
            self.frozen_by = Some(caller.to_string());
            emit("frozen", json!({ "by": caller }));
        }
    }

    /// Only the owner can release the brake (full-access key required).
    #[payable]
    pub fn unfreeze(&mut self) {
        self.assert_owner();
        self.frozen = false;
        self.frozen_by = None;
        self.recent_blocks_ms.clear();
        emit("unfrozen", json!({}));
    }

    #[payable]
    pub fn add_guardians(&mut self, account_ids: Vec<AccountId>) {
        self.assert_owner();
        for a in &account_ids {
            self.guardians.insert(a.clone());
        }
        emit("guardians_added", json!({ "account_ids": account_ids }));
    }

    #[payable]
    pub fn remove_guardians(&mut self, account_ids: Vec<AccountId>) {
        self.assert_owner();
        for a in &account_ids {
            self.guardians.remove(a);
        }
        emit("guardians_removed", json!({ "account_ids": account_ids }));
    }

    /// Set (or with `null`, remove) the auto-freeze tripwire.
    #[payable]
    pub fn set_tripwire(&mut self, tripwire: Option<Tripwire>) {
        self.assert_owner();
        if let Some(t) = &tripwire {
            validate_tripwire(t);
        }
        self.tripwire = tripwire.clone();
        self.recent_blocks_ms.clear();
        emit("tripwire_updated", json!({ "tripwire": tripwire }));
    }

    /// Start a fresh spending window now (spent counter back to zero).
    #[payable]
    pub fn reset_window(&mut self) {
        self.assert_owner();
        self.window_start_ms = env::block_timestamp_ms();
        self.spent_in_window = NearToken::from_yoctonear(0);
        emit("window_reset", json!({}));
    }

    /// Approve a pending payment. The owner's decision overrides the window cap,
    /// but the funds must still be available.
    #[payable]
    pub fn approve(&mut self, id: u64) -> Attempt {
        self.assert_owner();
        let mut attempt = self.take_pending(id);
        require!(
            attempt.amount <= self.available_balance(),
            "Not enough available balance"
        );
        self.roll_window();
        attempt.outcome = Outcome::Approved;
        self.spent_in_window = self.spent_in_window.saturating_add(attempt.amount);
        self.total_sent += 1;
        Promise::new(attempt.receiver_id.clone()).transfer(attempt.amount).detach();
        self.record(&attempt, "payment_approved");
        attempt
    }

    #[payable]
    pub fn reject(&mut self, id: u64) -> Attempt {
        self.assert_owner();
        let mut attempt = self.take_pending(id);
        attempt.outcome = Outcome::Rejected;
        self.record(&attempt, "payment_rejected");
        attempt
    }

    /// Move funds back to the owner.
    #[payable]
    pub fn withdraw(&mut self, amount: NearToken) {
        self.assert_owner();
        require!(amount <= self.available_balance(), "Not enough available balance");
        Promise::new(self.owner_id.clone()).transfer(amount).detach();
        emit("withdrawn", json!({ "amount": amount }));
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

    pub fn get_status(&self) -> Status {
        let (spent, resets_at) = self.current_window();
        Status {
            owner_id: self.owner_id.clone(),
            agent_id: self.agent_id.clone(),
            frozen: self.frozen,
            policy: self.policy.clone(),
            spent_in_window: spent,
            window_resets_at_ms: resets_at,
            available_balance: self.available_balance(),
            total_sent: self.total_sent,
            total_blocked: self.total_blocked,
            pending_count: self.pending.len() as u32,
            frozen_by: self.frozen_by.clone(),
            tripwire: self.tripwire.clone(),
            strikes: self.current_strikes(),
            guardian_count: self.guardians.len(),
        }
    }

    /// Most recent attempts first.
    pub fn get_history(&self, limit: Option<u32>) -> Vec<Attempt> {
        let n = limit.unwrap_or(MAX_HISTORY as u32) as usize;
        self.history.iter().rev().take(n).cloned().collect()
    }

    pub fn get_pending(&self) -> Vec<Attempt> {
        self.pending.clone()
    }

    pub fn get_allowlist(&self) -> Vec<AccountId> {
        self.allowlist.iter().cloned().collect()
    }

    pub fn get_blocklist(&self) -> Vec<AccountId> {
        self.blocklist.iter().cloned().collect()
    }

    pub fn get_guardians(&self) -> Vec<AccountId> {
        self.guardians.iter().cloned().collect()
    }

    // ------------------------------------------------------------------
    // Internal
    // ------------------------------------------------------------------

    /// Record a blocked attempt as a strike; freeze if the tripwire trips.
    fn strike(&mut self) {
        let Some(tw) = self.tripwire.clone() else { return };
        let now = env::block_timestamp_ms();
        let period = tw.period_sec.saturating_mul(1000);
        self.recent_blocks_ms.retain(|t| now.saturating_sub(*t) < period);
        self.recent_blocks_ms.push(now);
        // Only the newest `max_blocked` timestamps ever matter: bound storage.
        let max = tw.max_blocked as usize;
        if self.recent_blocks_ms.len() > max {
            let extra = self.recent_blocks_ms.len() - max;
            self.recent_blocks_ms.drain(..extra);
        }
        let strikes = self.recent_blocks_ms.len() as u32;
        if strikes >= tw.max_blocked && !self.frozen {
            self.frozen = true;
            self.frozen_by = Some("tripwire".to_string());
            emit(
                "auto_frozen",
                json!({ "strikes": strikes, "max_blocked": tw.max_blocked, "period_sec": tw.period_sec }),
            );
        }
    }

    fn current_strikes(&self) -> u32 {
        let Some(tw) = &self.tripwire else { return 0 };
        let now = env::block_timestamp_ms();
        let period = tw.period_sec.saturating_mul(1000);
        self.recent_blocks_ms
            .iter()
            .filter(|t| now.saturating_sub(**t) < period)
            .count() as u32
    }

    /// Err(reason) = block. Ok(true) = needs approval. Ok(false) = send now.
    fn check(&self, a: &Attempt) -> Result<bool, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if a.amount.is_zero() {
            return Err("zero_amount");
        }
        if a.receiver_id == env::current_account_id() {
            return Err("self_payment");
        }
        if self.blocklist.contains(&a.receiver_id) {
            return Err("blocklisted_recipient");
        }
        if self.policy.allowlist_only && !self.allowlist.contains(&a.receiver_id) {
            return Err("recipient_not_allowlisted");
        }
        if a.amount > self.policy.max_per_tx {
            return Err("over_per_tx_limit");
        }
        if self.spent_in_window.saturating_add(a.amount) > self.policy.window_limit {
            return Err("over_window_limit");
        }
        if a.amount > self.policy.auto_approve_limit {
            if self.pending.len() >= MAX_PENDING {
                return Err("too_many_pending");
            }
            return Ok(true);
        }
        if a.amount > self.available_balance() {
            return Err("insufficient_funds");
        }
        Ok(false)
    }

    fn assert_owner(&self) {
        assert_one_yocto();
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only the owner can do this"
        );
    }

    fn window_ms(&self) -> u64 {
        self.policy.window_sec.saturating_mul(1000)
    }

    /// (spent, resets_at) for the window as of now, without mutating state.
    fn current_window(&self) -> (NearToken, u64) {
        let now = env::block_timestamp_ms();
        let len = self.window_ms();
        if now.saturating_sub(self.window_start_ms) >= len {
            (NearToken::from_yoctonear(0), now + len)
        } else {
            (self.spent_in_window, self.window_start_ms + len)
        }
    }

    fn roll_window(&mut self) {
        let now = env::block_timestamp_ms();
        if now.saturating_sub(self.window_start_ms) >= self.window_ms() {
            self.window_start_ms = now;
            self.spent_in_window = NearToken::from_yoctonear(0);
        }
    }

    fn available_balance(&self) -> NearToken {
        let locked = env::storage_byte_cost().saturating_mul(env::storage_usage() as u128);
        env::account_balance()
            .saturating_sub(locked)
            .saturating_sub(BALANCE_RESERVE)
    }

    fn take_pending(&mut self, id: u64) -> Attempt {
        let idx = self
            .pending
            .iter()
            .position(|p| p.id == id)
            .unwrap_or_else(|| env::panic_str("No pending payment with that id"));
        self.pending.remove(idx)
    }

    fn record(&mut self, a: &Attempt, event: &str) {
        if self.history.len() >= MAX_HISTORY {
            self.history.remove(0);
        }
        self.history.push(a.clone());
        emit(event, serde_json::to_value(a).unwrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{get_logs, VMContextBuilder};
    use near_sdk::testing_env;

    const SEC_NS: u64 = 1_000_000_000;

    fn owner() -> AccountId {
        "owner.testnet".parse().unwrap()
    }
    fn agent() -> AccountId {
        "agent.testnet".parse().unwrap()
    }
    fn api() -> AccountId {
        "api-provider.testnet".parse().unwrap()
    }
    fn scammer() -> AccountId {
        "totally-legit-refund.testnet".parse().unwrap()
    }
    fn contract() -> AccountId {
        "leash.testnet".parse().unwrap()
    }
    fn near(n: u128) -> NearToken {
        NearToken::from_near(n)
    }
    fn milli(n: u128) -> NearToken {
        NearToken::from_millinear(n)
    }

    fn policy() -> Policy {
        Policy {
            max_per_tx: near(2),
            auto_approve_limit: milli(500), // 0.5 NEAR
            window_limit: near(3),
            window_sec: 86_400,
            allowlist_only: true,
        }
    }

    fn ctx(caller: AccountId, deposit: u128, ts_sec: u64) {
        testing_env!(VMContextBuilder::new()
            .current_account_id(contract())
            .predecessor_account_id(caller.clone())
            .signer_account_id(caller)
            .account_balance(near(10))
            .attached_deposit(NearToken::from_yoctonear(deposit))
            .block_timestamp(ts_sec * SEC_NS)
            .build());
    }

    fn setup() -> AgentLeash {
        ctx(owner(), 0, 1_000);
        AgentLeash::new(owner(), agent(), policy(), Some(vec![api()]), None, None)
    }

    fn guardian() -> AccountId {
        "guardian-bot.testnet".parse().unwrap()
    }

    /// Leash with a tripwire of 3 blocks per hour and one guardian.
    fn setup_guarded() -> AgentLeash {
        ctx(owner(), 0, 1_000);
        AgentLeash::new(
            owner(),
            agent(),
            policy(),
            Some(vec![api()]),
            Some(vec![guardian()]),
            Some(Tripwire { max_blocked: 3, period_sec: 3600 }),
        )
    }

    #[test]
    fn small_payment_to_allowlisted_is_sent() {
        let mut c = setup();
        ctx(agent(), 0, 1_001);
        let a = c.pay(api(), milli(100), Some("API credits".into()));
        assert_eq!(a.outcome, Outcome::Sent);
        assert_eq!(c.get_status().spent_in_window, milli(100));
        assert!(get_logs().iter().any(|l| l.contains("\"payment_sent\"")));
    }

    #[test]
    fn unknown_recipient_is_blocked_and_logged() {
        let mut c = setup();
        ctx(agent(), 0, 1_001);
        let a = c.pay(scammer(), milli(100), Some("refund fee".into()));
        assert_eq!(a.outcome, Outcome::Blocked);
        assert_eq!(a.reason.as_deref(), Some("recipient_not_allowlisted"));
        assert_eq!(c.get_status().spent_in_window, milli(0));
        assert_eq!(c.get_status().total_blocked, 1);
        assert_eq!(c.get_history(None)[0].receiver_id, scammer());
        assert!(get_logs().iter().any(|l| l.contains("\"payment_blocked\"")));
    }

    #[test]
    fn blocklist_wins_even_without_allowlist_mode() {
        let mut c = setup();
        ctx(owner(), 1, 1_001);
        c.set_policy(Policy { allowlist_only: false, ..policy() });
        c.block(vec![scammer()]);
        ctx(agent(), 0, 1_002);
        let a = c.pay(scammer(), milli(100), None);
        assert_eq!(a.reason.as_deref(), Some("blocklisted_recipient"));
        // Anyone else is fine when allowlist mode is off.
        let b = c.pay("random.testnet".parse().unwrap(), milli(100), None);
        assert_eq!(b.outcome, Outcome::Sent);
    }

    #[test]
    fn single_large_drain_is_blocked() {
        let mut c = setup();
        ctx(agent(), 0, 1_001);
        let a = c.pay(api(), near(9), None);
        assert_eq!(a.reason.as_deref(), Some("over_per_tx_limit"));
    }

    #[test]
    fn many_small_payments_hit_window_cap_then_reset() {
        let mut c = setup();
        ctx(agent(), 0, 1_001);
        let mut sent = 0;
        let mut last = None;
        for _ in 0..10 {
            let a = c.pay(api(), milli(500), None);
            if a.outcome == Outcome::Sent {
                sent += 1;
            }
            last = Some(a);
        }
        assert_eq!(sent, 6); // 6 x 0.5 = 3 NEAR window limit
        assert_eq!(last.unwrap().reason.as_deref(), Some("over_window_limit"));

        // A day later the window resets.
        ctx(agent(), 0, 1_001 + 86_400);
        assert_eq!(c.get_status().spent_in_window, milli(0));
        assert_eq!(c.pay(api(), milli(500), None).outcome, Outcome::Sent);
    }

    #[test]
    fn owner_can_reset_window() {
        let mut c = setup();
        ctx(agent(), 0, 1_001);
        c.pay(api(), milli(500), None);
        ctx(owner(), 1, 1_002);
        c.reset_window();
        assert_eq!(c.get_status().spent_in_window, milli(0));
    }

    #[test]
    #[should_panic(expected = "Only the owner can do this")]
    fn agent_cannot_reset_window() {
        let mut c = setup();
        ctx(agent(), 1, 1_001);
        c.reset_window();
    }

    #[test]
    fn medium_payment_waits_for_owner_approval() {
        let mut c = setup();
        ctx(agent(), 0, 1_001);
        let a = c.pay(api(), near(1), Some("GPU hour".into()));
        assert_eq!(a.outcome, Outcome::Pending);
        assert_eq!(c.get_pending().len(), 1);
        assert_eq!(c.get_status().spent_in_window, milli(0));

        ctx(owner(), 1, 1_002);
        let approved = c.approve(a.id);
        assert_eq!(approved.outcome, Outcome::Approved);
        assert!(c.get_pending().is_empty());
        assert_eq!(c.get_status().spent_in_window, near(1));
    }

    #[test]
    fn owner_can_reject_pending() {
        let mut c = setup();
        ctx(agent(), 0, 1_001);
        let a = c.pay(api(), near(1), None);
        ctx(owner(), 1, 1_002);
        assert_eq!(c.reject(a.id).outcome, Outcome::Rejected);
        assert!(c.get_pending().is_empty());
        assert_eq!(c.get_status().spent_in_window, milli(0));
    }

    #[test]
    fn freeze_blocks_everything_until_unfreeze() {
        let mut c = setup();
        ctx(owner(), 1, 1_001);
        c.freeze();
        ctx(agent(), 0, 1_002);
        assert_eq!(c.pay(api(), milli(10), None).reason.as_deref(), Some("frozen"));
        ctx(owner(), 1, 1_003);
        c.unfreeze();
        ctx(agent(), 0, 1_004);
        assert_eq!(c.pay(api(), milli(10), None).outcome, Outcome::Sent);
    }

    #[test]
    fn insufficient_funds_is_blocked() {
        let mut c = setup();
        ctx(owner(), 1, 1_001);
        c.set_policy(Policy {
            max_per_tx: near(100),
            auto_approve_limit: near(100),
            window_limit: near(100),
            ..policy()
        });
        ctx(agent(), 0, 1_002);
        // Balance is 10 NEAR minus storage and reserve.
        let a = c.pay(api(), near(10), None);
        assert_eq!(a.reason.as_deref(), Some("insufficient_funds"));
    }

    #[test]
    fn zero_and_self_payments_are_blocked() {
        let mut c = setup();
        ctx(agent(), 0, 1_001);
        assert_eq!(c.pay(api(), milli(0), None).reason.as_deref(), Some("zero_amount"));
        ctx(owner(), 1, 1_002);
        c.allow(vec![contract()]);
        ctx(agent(), 0, 1_003);
        assert_eq!(c.pay(contract(), milli(1), None).reason.as_deref(), Some("self_payment"));
    }

    #[test]
    #[should_panic(expected = "Only the agent can call pay")]
    fn only_agent_can_pay() {
        let mut c = setup();
        ctx(scammer(), 0, 1_001);
        c.pay(scammer(), milli(1), None);
    }

    #[test]
    #[should_panic(expected = "Only the owner can do this")]
    fn agent_cannot_change_policy() {
        let mut c = setup();
        ctx(agent(), 1, 1_001);
        c.set_policy(Policy { max_per_tx: near(1000), ..policy() });
    }

    #[test]
    #[should_panic(expected = "Requires attached deposit of exactly 1 yoctoNEAR")]
    fn owner_actions_need_one_yocto() {
        let mut c = setup();
        ctx(owner(), 1, 1_001);
        c.freeze();
        ctx(owner(), 0, 1_002);
        c.unfreeze();
    }

    #[test]
    #[should_panic(expected = "auto_approve_limit must be <= max_per_tx")]
    fn invalid_policy_is_rejected() {
        let mut c = setup();
        ctx(owner(), 1, 1_001);
        c.set_policy(Policy { auto_approve_limit: near(5), ..policy() });
    }

    #[test]
    fn history_is_capped_and_newest_first() {
        let mut c = setup();
        ctx(agent(), 0, 1_001);
        for _ in 0..(MAX_HISTORY + 5) {
            c.pay(scammer(), milli(1), None);
        }
        let h = c.get_history(None);
        assert_eq!(h.len(), MAX_HISTORY);
        assert_eq!(h[0].id, (MAX_HISTORY + 4) as u64);
        assert_eq!(c.get_history(Some(3)).len(), 3);
    }

    #[test]
    fn long_memo_is_truncated() {
        let mut c = setup();
        ctx(agent(), 0, 1_001);
        let a = c.pay(api(), milli(1), Some("x".repeat(500)));
        assert_eq!(a.memo.len(), MAX_MEMO_LEN);
    }

    // ---------------- tripwire ----------------

    #[test]
    fn tripwire_auto_freezes_after_three_strikes() {
        let mut c = setup_guarded();
        ctx(agent(), 0, 1_001);
        c.pay(scammer(), milli(100), None); // strike 1
        assert_eq!(c.get_status().strikes, 1);
        c.pay(api(), near(9), None); // strike 2
        assert!(!c.get_status().frozen);
        c.pay(scammer(), milli(1), None); // strike 3 -> freeze
        let s = c.get_status();
        assert!(s.frozen);
        assert_eq!(s.frozen_by.as_deref(), Some("tripwire"));
        assert!(get_logs().iter().any(|l| l.contains("\"auto_frozen\"")));
        // Even a perfectly valid payment is now refused.
        assert_eq!(c.pay(api(), milli(10), None).reason.as_deref(), Some("frozen"));
    }

    #[test]
    fn tripwire_forgets_old_strikes() {
        let mut c = setup_guarded();
        ctx(agent(), 0, 1_001);
        c.pay(scammer(), milli(1), None);
        c.pay(scammer(), milli(1), None);
        // Over an hour later, the old strikes have expired.
        ctx(agent(), 0, 1_001 + 3_601);
        assert_eq!(c.get_status().strikes, 0);
        c.pay(scammer(), milli(1), None);
        assert!(!c.get_status().frozen);
        assert_eq!(c.get_status().strikes, 1);
    }

    #[test]
    fn frozen_attempts_do_not_count_and_unfreeze_clears_strikes() {
        let mut c = setup_guarded();
        ctx(agent(), 0, 1_001);
        for _ in 0..3 {
            c.pay(scammer(), milli(1), None);
        }
        for _ in 0..5 {
            c.pay(api(), milli(1), None); // blocked as "frozen"
        }
        assert_eq!(c.get_status().strikes, 3);
        ctx(owner(), 1, 1_002);
        c.unfreeze();
        let s = c.get_status();
        assert!(!s.frozen);
        assert_eq!(s.strikes, 0);
        assert_eq!(s.frozen_by, None);
    }

    #[test]
    fn no_tripwire_means_no_auto_freeze() {
        let mut c = setup();
        ctx(agent(), 0, 1_001);
        for _ in 0..10 {
            c.pay(scammer(), milli(1), None);
        }
        assert!(!c.get_status().frozen);
    }

    #[test]
    #[should_panic(expected = "max_blocked must be > 0")]
    fn invalid_tripwire_is_rejected() {
        let mut c = setup();
        ctx(owner(), 1, 1_001);
        c.set_tripwire(Some(Tripwire { max_blocked: 0, period_sec: 60 }));
    }

    // ---------------- guardians ----------------

    #[test]
    fn guardian_can_freeze_without_deposit() {
        let mut c = setup_guarded();
        ctx(guardian(), 0, 1_001);
        c.freeze();
        let s = c.get_status();
        assert!(s.frozen);
        assert_eq!(s.frozen_by.as_deref(), Some("guardian-bot.testnet"));
    }

    #[test]
    #[should_panic(expected = "Only the owner can do this")]
    fn guardian_cannot_unfreeze() {
        let mut c = setup_guarded();
        ctx(guardian(), 0, 1_001);
        c.freeze();
        ctx(guardian(), 1, 1_002);
        c.unfreeze();
    }

    #[test]
    #[should_panic(expected = "Only the owner or a guardian can freeze")]
    fn stranger_cannot_freeze() {
        let mut c = setup_guarded();
        ctx(scammer(), 0, 1_001);
        c.freeze();
    }

    #[test]
    #[should_panic(expected = "Only the owner can do this")]
    fn agent_cannot_add_guardians() {
        let mut c = setup();
        ctx(agent(), 1, 1_001);
        c.add_guardians(vec![agent()]);
    }

    #[test]
    fn owner_manages_guardians() {
        let mut c = setup();
        ctx(owner(), 1, 1_001);
        c.add_guardians(vec![guardian()]);
        assert_eq!(c.get_guardians(), vec![guardian()]);
        c.remove_guardians(vec![guardian()]);
        assert!(c.get_guardians().is_empty());
    }

    // ---------------- upgrade ----------------

    #[test]
    fn migrate_keeps_v1_data() {
        ctx(owner(), 0, 1_000);
        let mut allow = IterableSet::new(StorageKey::Allowlist);
        allow.insert(api());
        allow.flush();
        let v1 = AgentLeashV1 {
            owner_id: owner(),
            agent_id: agent(),
            policy: policy(),
            frozen: true,
            allowlist: allow,
            blocklist: IterableSet::new(StorageKey::Blocklist),
            window_start_ms: 1_000_000,
            spent_in_window: milli(450),
            next_id: 27,
            history: vec![],
            pending: vec![],
            total_sent: 11,
            total_blocked: 9,
        };
        env::state_write(&v1);
        let c = AgentLeash::migrate();
        let s = c.get_status();
        assert_eq!(s.total_sent, 11);
        assert_eq!(s.total_blocked, 9);
        assert!(s.frozen);
        assert_eq!(s.frozen_by.as_deref(), Some("owner.testnet"));
        assert_eq!(s.tripwire, None);
        assert_eq!(c.get_allowlist(), vec![api()]);
    }
}
