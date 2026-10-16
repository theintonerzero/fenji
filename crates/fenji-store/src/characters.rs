use sqlx::sqlite::SqliteRow;
use sqlx::{FromRow, Row};

use crate::error::is_unique;
use crate::{Error, Result, Store};

/// The level characters are created at: the client's `CreateLv`, which their starting weapons
/// need (M4 world spec §2.5).
pub const CREATE_LEVEL: i64 = 37;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCharacter {
    pub name: String,
    pub vocation: i64,
    pub skeleton: i64,
    /// The appearance fields, as the server encodes them (M3 spec D8).
    pub appearance: Vec<u8>,
}

/// Where a character stands (M4 world spec §3.2): a scene, a point (z is up) and a yaw in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub scene: i64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Character {
    pub pid: i64,
    pub account_id: i64,
    pub name: String,
    pub vocation: i64,
    pub skeleton: i64,
    pub level: i64,
    pub appearance: Vec<u8>,
    pub created_at: i64,
    /// Where it last stood in the world; `None` until it first enters.
    pub place: Option<Place>,
}

/// The columns a [`Character`] is read from, as a literal for `concat!`: sqlx takes only static
/// SQL.
macro_rules! columns {
    () => {
        "pid, account_id, name, vocation, skeleton, level, appearance, created_at, \
         scene, pos_x, pos_y, pos_z, yaw"
    };
}

impl FromRow<'_, SqliteRow> for Character {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        let scene: Option<i64> = row.try_get("scene")?;
        let point: (Option<f64>, Option<f64>, Option<f64>, Option<f64>) = (
            row.try_get("pos_x")?,
            row.try_get("pos_y")?,
            row.try_get("pos_z")?,
            row.try_get("yaw")?,
        );
        let place = match (scene, point) {
            (Some(scene), (Some(x), Some(y), Some(z), Some(yaw))) => Some(Place {
                scene,
                x,
                y,
                z,
                yaw,
            }),
            _ => None,
        };
        Ok(Self {
            pid: row.try_get("pid")?,
            account_id: row.try_get("account_id")?,
            name: row.try_get("name")?,
            vocation: row.try_get("vocation")?,
            skeleton: row.try_get("skeleton")?,
            level: row.try_get("level")?,
            appearance: row.try_get("appearance")?,
            created_at: row.try_get("created_at")?,
            place,
        })
    }
}

impl Store {
    /// The account's characters, oldest first.
    pub async fn list_characters(&self, account_id: i64) -> Result<Vec<Character>> {
        Ok(sqlx::query_as(concat!(
            "SELECT ",
            columns!(),
            " FROM characters WHERE account_id = ? ORDER BY pid"
        ))
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// The character `pid`, only if it belongs to `account_id`.
    pub async fn character(&self, account_id: i64, pid: i64) -> Result<Option<Character>> {
        Ok(sqlx::query_as(concat!(
            "SELECT ",
            columns!(),
            " FROM characters WHERE pid = ? AND account_id = ?"
        ))
        .bind(pid)
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Creates a character at [`CREATE_LEVEL`], with no place yet. The account's limit and the
    /// name check run in the same transaction as the insert.
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
        let inserted = sqlx::query_as(concat!(
            "INSERT INTO characters \
             (account_id, name, vocation, skeleton, level, appearance, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING ",
            columns!()
        ))
        .bind(account_id)
        .bind(&new.name)
        .bind(new.vocation)
        .bind(new.skeleton)
        .bind(CREATE_LEVEL)
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

    /// Saves where `pid` stands, in one statement. False when the character is gone (deleted
    /// while its player was in the world). SQLite would store a NaN as null and lose the place,
    /// so a place that isn't finite is refused.
    pub async fn save_place(&self, pid: i64, place: &Place) -> Result<bool> {
        if ![place.x, place.y, place.z, place.yaw]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err(Error::NotFinite);
        }
        let r = sqlx::query(
            "UPDATE characters SET scene = ?, pos_x = ?, pos_y = ?, pos_z = ?, yaw = ? \
             WHERE pid = ?",
        )
        .bind(place.scene)
        .bind(place.x)
        .bind(place.y)
        .bind(place.z)
        .bind(place.yaw)
        .bind(pid)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() == 1)
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
    use super::{CREATE_LEVEL, NewCharacter, Place};
    use crate::Error;
    use crate::test_util::{NOW, account, store, warm};

    fn new(name: &str) -> NewCharacter {
        NewCharacter {
            name: name.into(),
            vocation: 1,
            skeleton: 2,
            appearance: vec![1, 2, 3],
        }
    }

    const PLACE: Place = Place {
        scene: 5,
        x: 2366.0,
        y: -71735.5,
        z: 64854.0,
        yaw: 345.0,
    };

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
                c.created_at,
                c.place
            ),
            ("Lin", CREATE_LEVEL, 1, 2, &[1u8, 2, 3][..], NOW, None)
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
        warm(&s).await;
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
        warm(&s).await;
        let (x, y) = tokio::join!(
            s.create_character(a, &one, 1, NOW),
            s.create_character(a, &two, 1, NOW)
        );
        assert_eq!(x.is_ok() as u8 + y.is_ok() as u8, 1);
        assert!(matches!(x.err().or(y.err()), Some(Error::TooMany)));
    }

    #[tokio::test]
    async fn a_place_is_saved_and_read_back_with_its_character() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        let c = s.create_character(a, &new("Lin"), 4, NOW).await.unwrap();
        assert!(s.save_place(c.pid, &PLACE).await.unwrap());
        let read = s.character(a, c.pid).await.unwrap().unwrap();
        assert_eq!(read.place, Some(PLACE));
        assert_eq!(s.list_characters(a).await.unwrap()[0].place, Some(PLACE));
        let moved = Place { x: 1.0, ..PLACE };
        assert!(s.save_place(c.pid, &moved).await.unwrap());
        assert_eq!(
            s.character(a, c.pid).await.unwrap().unwrap().place,
            Some(moved)
        );
    }

    #[tokio::test]
    async fn character_finds_only_the_accounts_own() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        let other = account(&s, 2, "eve").await;
        let c = s.create_character(a, &new("Lin"), 4, NOW).await.unwrap();
        assert_eq!(s.character(a, c.pid).await.unwrap(), Some(c.clone()));
        assert_eq!(s.character(other, c.pid).await.unwrap(), None);
        assert_eq!(s.character(a, c.pid + 1).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_place_that_is_not_finite_is_refused_and_a_gone_character_is_not_saved() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        let c = s.create_character(a, &new("Lin"), 4, NOW).await.unwrap();
        for bad in [
            Place {
                x: f64::NAN,
                ..PLACE
            },
            Place {
                y: f64::INFINITY,
                ..PLACE
            },
            Place {
                yaw: f64::NEG_INFINITY,
                ..PLACE
            },
        ] {
            assert!(matches!(
                s.save_place(c.pid, &bad).await,
                Err(Error::NotFinite)
            ));
        }
        assert_eq!(s.character(a, c.pid).await.unwrap().unwrap().place, None);
        assert!(s.delete_character(a, c.pid).await.unwrap());
        assert!(!s.save_place(c.pid, &PLACE).await.unwrap());
    }

    #[tokio::test]
    async fn an_m3_database_gets_level_37_and_no_place() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fenji.db");
        // A database as v0.4.1 left it: the first migration only, and a character at level 1.
        let opts = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new().connect_with(opts).await.unwrap();
        sqlx::migrate!("./migrations")
            .run_to(1, &pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO accounts (username, password_phc, created_at) VALUES ('bob', '$phc', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO characters (account_id, name, vocation, skeleton, appearance, created_at) \
             VALUES (1, 'Lin', 0, 2, x'01', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
        let s = crate::Store::open(&path).await.unwrap();
        let list = s.list_characters(1).await.unwrap();
        assert_eq!(
            (list[0].name.as_str(), list[0].level, list[0].place),
            ("Lin", CREATE_LEVEL, None)
        );
    }
}
