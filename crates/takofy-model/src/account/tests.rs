use super::{Account, fingerprint};

#[test]
fn the_fingerprint_is_fnv_1a_64() {
    assert_eq!(fingerprint(""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fingerprint("a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fingerprint("foobar"), 0x8594_4171_f739_67e8);
    assert_eq!(
        Account::from_wire("foobar").map(|account| account.key()),
        Some(0x8594_4171_f739_67e8)
    );
}

#[test]
fn a_blank_name_names_no_account() {
    assert_eq!(Account::from_wire(""), None);
    assert_eq!(Account::from_wire(" \t\n"), None);
}

#[test]
fn a_padded_name_is_the_name_it_pads() {
    assert_eq!(Account::from_wire(" tako "), Account::from_wire("tako"));
}

#[test]
fn the_same_name_is_the_same_account_and_a_different_one_is_not() {
    assert_eq!(Account::from_wire("tako"), Account::from_wire("tako"));
    assert_ne!(Account::from_wire("tako"), Account::from_wire("tako2"));
}

#[test]
fn the_debug_line_names_no_account_and_no_key() {
    let account = Account::from_wire("tako").expect("a name");
    assert_eq!(format!("{account:?}"), "Account(..)");
}

#[test]
fn a_key_reads_back_as_the_account_it_came_from() {
    let account = Account::from_wire("tako").expect("a name");
    assert_eq!(Account::from_key(account.key()), account);
}
