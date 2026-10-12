use sqlx::FromRow;

use crate::error::is_unique;
use crate::{Error, Result, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCharacter {
    pub name: String,
    pub vocation: i64,
    pub skeleton: i64,
    /// The appearance fields, as the server encodes them (M3 spec D8).
    pub appearance: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Character {
    pub pid: i64,
    pub account_id: i64,
    pub name: String,
    pub vocation: i64,
    pub skeleton: i64,
    pub level: i64,
    pub appearance: Vec<u8>,
    pub created_at: i64,
}

impl Store {
    /// The account's characters, oldest first.
    pub async fn list_characters(&self, account_id: i64) -> Result<Vec<Character>> {
        Ok(sqlx::query_as(
            "SELECT pid, account_id, name, vocation, skeleton, level, appearance, created_at \
             FROM characters WHERE account_id = ? ORDER BY pid",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Creates a character at level 1. The account's limit and the name check run in the same
    /// transaction as the insert.
    pub async fn create_character(
        &self,
        account_id: i64,
        new: &NewCharacter,
        max_per_account: u32,
        now: i64,
    ) -> Result<Character> {
        let mut tx = self.begin().await?;
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM characters WHERE account_id = ?")
                .bind(account_id)
                .fetch_one(&mut *tx)
                .await?;
        if count >= i64::from(max_per_account) {
            return Err(Error::TooMany);
        }
        let inserted = sqlx::query_as(
            "INSERT INTO characters (account_id, name, vocation, skeleton, appearance, created_at) \
             VALUES (?, ?, ?, ?, ?, ?) \
             RETURNING pid, account_id, name, vocation, skeleton, level, appearance, created_at",
        )
        .bind(account_id)
        .bind(&new.name)
        .bind(new.vocation)
        .bind(new.skeleton)
        .bind(&new.appearance)
        .bind(now)
        .fetch_one(&mut *tx)
        .await;
        let c: Character = match inserted {
            Ok(c) => c,
            Err(e) if is_unique(&e) => return Err(Error::NameTaken),
            Err(e) => return Err(e.into()),
        };
        tx.commit().await?;
        Ok(c)
    }

    /// Deletes `pid` only if it belongs to `account_id`.
    pub async fn delete_character(&self, account_id: i64, pid: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM characters WHERE pid = ? AND account_id = ?")
            .bind(pid)
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::NewCharacter;
    use crate::Error;
    use crate::test_util::{NOW, account, store};

    fn new(name: &str) -> NewCharacter {
        NewCharacter {
            name: name.into(),
            vocation: 1,
            skeleton: 2,
            appearance: vec![1, 2, 3],
        }
    }

    #[tokio::test]
    async fn create_list_and_delete() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        let c = s.create_character(a, &new("Lin"), 4, NOW).await.unwrap();
        assert_eq!(
            (
                c.name.as_str(),
                c.level,
                c.vocation,
                c.skeleton,
                c.appearance.as_slice(),
                c.created_at
            ),
            ("Lin", 1, 1, 2, &[1u8, 2, 3][..], NOW)
        );
        assert!(c.pid > 0);
        assert_eq!(s.list_characters(a).await.unwrap(), vec![c.clone()]);
        let other = account(&s, 2, "eve").await;
        assert!(
            !s.delete_character(other, c.pid).await.unwrap(),
            "not eve's"
        );
        assert!(s.delete_character(a, c.pid).await.unwrap());
        assert!(s.list_characters(a).await.unwrap().is_empty());
        let again = s.create_character(a, &new("Lin"), 4, NOW).await.unwrap();
        assert!(again.pid > c.pid, "a pid is never reused");
    }

    #[tokio::test]
    async fn names_are_unique_case_insensitively_and_accounts_are_capped() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        let b = account(&s, 2, "eve").await;
        s.create_character(a, &new("Lin"), 2, NOW).await.unwrap();
        assert!(matches!(
            s.create_character(b, &new("LIN"), 2, NOW).await,
            Err(Error::NameTaken)
        ));
        s.create_character(a, &new("Two"), 2, NOW).await.unwrap();
        assert!(matches!(
            s.create_character(a, &new("Three"), 2, NOW).await,
            Err(Error::TooMany)
        ));
        assert_eq!(s.list_characters(a).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn concurrent_creates_with_one_name_store_one() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        let b = account(&s, 2, "eve").await;
        let (one, two) = (new("Same"), new("same"));
        let (x, y) = tokio::join!(
            s.create_character(a, &one, 4, NOW),
            s.create_character(b, &two, 4, NOW)
        );
        assert_eq!(x.is_ok() as u8 + y.is_ok() as u8, 1);
        assert!(matches!(x.err().or(y.err()), Some(Error::NameTaken)));
    }

    #[tokio::test]
    async fn concurrent_creates_on_one_account_respect_its_limit() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        let (one, two) = (new("One"), new("Two"));
        let (x, y) = tokio::join!(
            s.create_character(a, &one, 1, NOW),
            s.create_character(a, &two, 1, NOW)
        );
        assert_eq!(x.is_ok() as u8 + y.is_ok() as u8, 1);
        assert!(matches!(x.err().or(y.err()), Some(Error::TooMany)));
    }
}
