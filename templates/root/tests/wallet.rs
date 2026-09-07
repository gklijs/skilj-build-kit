//! Integration tests for the `wallet` bounded context — the pattern every
//! `build-state-change` slice's own `tests/<context_snake_case>.rs` follows
//! (`.build-kit/CLAUDE.md`'s "Slice shape" section), applied to this kit's
//! own worked example so that pattern actually exists somewhere runnable,
//! not just described in prose.
//!
//! Pure `decide()` unit tests first, no Postgres needed — `decide()` takes
//! nothing but `&Payload`/`&[Event]`, so it's called directly. See
//! `.claude/skills/build-state-change/SKILL.md` Step 7 for why this is the
//! pattern (and for the `tag_mappings()` tests below it).

use my_app::wallet::{
    Deposit, DepositPayload, Deposited, DepositedPayload, WalletEvent, Withdraw, WithdrawPayload,
    Withdrawn, WithdrawnPayload,
};
use skilj::{CommandType, EventType};
use skilj_core::shared::{CommandDecision, TagMapping};

// --- Deposit::decide ---

#[test]
fn deposit_of_a_positive_amount_is_accepted() {
    let payload = DepositPayload { wallet_id: "w1".into(), amount: 100 };
    let decision = Deposit::decide(&payload, &[]);
    assert!(matches!(decision, CommandDecision::Accepted { events } if events.len() == 1));
}

#[test]
fn deposit_of_a_non_positive_amount_is_rejected() {
    for amount in [0, -1] {
        let payload = DepositPayload { wallet_id: "w1".into(), amount };
        let decision = Deposit::decide(&payload, &[]);
        assert!(matches!(decision, CommandDecision::Rejected { kind, .. } if kind == "invalid_amount"));
    }
}

// --- Withdraw::decide ---

#[test]
fn withdraw_of_a_non_positive_amount_is_rejected() {
    for amount in [0, -1] {
        let payload = WithdrawPayload { wallet_id: "w1".into(), amount };
        let decision = Withdraw::decide(&payload, &[]);
        assert!(matches!(decision, CommandDecision::Rejected { kind, .. } if kind == "invalid_amount"));
    }
}

#[test]
fn withdraw_more_than_the_folded_balance_is_rejected_insufficient_funds() {
    let matching_events = vec![WalletEvent::Deposited(DepositedPayload { wallet_id: "w1".into(), amount: 50 })];
    let payload = WithdrawPayload { wallet_id: "w1".into(), amount: 100 };
    let decision = Withdraw::decide(&payload, &matching_events);
    assert!(matches!(decision, CommandDecision::Rejected { kind, .. } if kind == "insufficient_funds"));
}

#[test]
fn withdraw_up_to_the_folded_balance_is_accepted() {
    let matching_events = vec![
        WalletEvent::Deposited(DepositedPayload { wallet_id: "w1".into(), amount: 100 }),
        WalletEvent::Withdrawn(WithdrawnPayload { wallet_id: "w1".into(), amount: 30 }),
    ];
    let payload = WithdrawPayload { wallet_id: "w1".into(), amount: 70 };
    let decision = Withdraw::decide(&payload, &matching_events);
    assert!(matches!(decision, CommandDecision::Accepted { events } if events.len() == 1));
}

// --- tag_mappings() — asserted directly, not just decide()'s outcome.
// See build-state-change/SKILL.md's own note on why: a wrong key/field name
// here passes every test above (they hand-build `matching_events` and never
// exercise the real tag-scoping path), and only breaks `matching_events`'
// scoping at runtime.

fn wallet_tag() -> Vec<TagMapping> {
    vec![TagMapping { key: "wallet".into(), field: "wallet_id".into() }]
}

#[test]
fn deposit_and_deposited_share_the_wallet_tag() {
    assert_eq!(Deposit::tag_mappings(), wallet_tag());
    assert_eq!(Deposited::tag_mappings(), wallet_tag());
}

#[test]
fn withdraw_and_withdrawn_share_the_wallet_tag() {
    assert_eq!(Withdraw::tag_mappings(), wallet_tag());
    assert_eq!(Withdrawn::tag_mappings(), wallet_tag());
}
