//! A person's forge connections (ADOPTION_PLAN X.27 step 1, PLAN.md T83):
//! kept per user, replaced per account, capped, never read across users,
//! gone with the account.

use harness_store::connections::{ConnectionPut, NewConnection};
use harness_store::{NewUser, Role, Store};

fn user(s: &Store, id: &str) {
    s.create_user(&NewUser {
        user_id: id.into(),
        username: id.into(),
        password_hash: "$argon2id$x".into(),
        role: Role::Member,
        exec_ceiling: harness_core::policy::ExecLevel::None,
        must_change_password: false,
    })
    .unwrap();
}

fn conn(id: &str, user: &str, host: &str, account: &str, sealed: &[u8]) -> NewConnection {
    NewConnection {
        connection_id: id.into(),
        user_id: user.into(),
        host: host.into(),
        account_id: account.into(),
        login: format!("login-{account}"),
        display_name: Some("A Person".into()),
        commit_email: None,
        scopes: Some(vec!["repo".into()]),
        scopes_more: 0,
        sealed_token: sealed.to_vec(),
    }
}

#[test]
fn per_account_replace_cap_and_privacy() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alice");
    user(&s, "bob");
    assert_eq!(
        s.connection_put(&conn("c1", "alice", "github.com", "1", b"one"), 2)
            .unwrap(),
        ConnectionPut::Added("c1".into())
    );
    // The same account again replaces the token and keeps the id.
    let mut again = conn("c-new", "alice", "github.com", "1", b"two");
    again.login = "renamed".into();
    again.scopes = None;
    assert_eq!(
        s.connection_put(&again, 2).unwrap(),
        ConnectionPut::Replaced("c1".into())
    );
    assert_eq!(s.connection_sealed("alice", "c1").unwrap().unwrap(), b"two");
    let row = s.connection("alice", "c1").unwrap().unwrap();
    assert_eq!((row.login.as_str(), row.scopes), ("renamed", None));
    // A second account, then the cap.
    s.connection_put(&conn("c2", "alice", "gitlab.com", "1", b"x"), 2)
        .unwrap();
    assert_eq!(
        s.connection_put(&conn("c3", "alice", "gitlab.com", "2", b"x"), 2)
            .unwrap(),
        ConnectionPut::OverCap
    );
    // At the cap a replacement still works.
    assert!(matches!(
        s.connection_put(&conn("c9", "alice", "gitlab.com", "1", b"y"), 2)
            .unwrap(),
        ConnectionPut::Replaced(_)
    ));
    // Bob's cap is his own; his row for the same forge account is his.
    s.connection_put(&conn("c4", "bob", "github.com", "1", b"b"), 2)
        .unwrap();
    assert_eq!(s.connections_of("alice").unwrap().len(), 2);
    assert_eq!(s.connections_of("bob").unwrap().len(), 1);
    // Another user's id reads, opens and deletes nothing.
    assert!(s.connection("bob", "c1").unwrap().is_none());
    assert!(s.connection_sealed("bob", "c1").unwrap().is_none());
    assert!(!s.connection_delete("bob", "c1").unwrap());
    assert!(s.connection("alice", "c1").unwrap().is_some());
    assert_eq!(s.sealed_count().unwrap(), 3);
    let samples = s.sealed_samples(2).unwrap();
    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0].sealed, b"two".to_vec());
    assert_eq!(samples[0].table, "user_connections");
    assert!(s.connection_delete("alice", "c1").unwrap());
    assert!(s.connection("alice", "c1").unwrap().is_none());
    assert_eq!(s.sealed_count().unwrap(), 2);
}

#[test]
fn deleting_the_account_removes_its_connections() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alice");
    user(&s, "bob");
    s.connection_put(&conn("c1", "alice", "github.com", "1", b"a"), 5)
        .unwrap();
    s.connection_put(&conn("c2", "bob", "github.com", "2", b"b"), 5)
        .unwrap();
    s.delete_user("alice").unwrap();
    assert!(s.connections_of("alice").unwrap().is_empty());
    assert!(s.connection_sealed("alice", "c1").unwrap().is_none());
    assert_eq!(s.connections_of("bob").unwrap().len(), 1);
    assert_eq!(s.sealed_count().unwrap(), 1);
}

#[test]
fn oversized_values_are_refused_by_the_table() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alice");
    let big = vec![0u8; 4097];
    assert!(
        s.connection_put(&conn("c1", "alice", "github.com", "1", &big), 5)
            .is_err()
    );
    let mut long = conn("c1", "alice", "github.com", "1", b"a");
    long.account_id = "9".repeat(33);
    assert!(s.connection_put(&long, 5).is_err());
}
