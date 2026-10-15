//! `sqlx::migrate!` embeds the migrations when the crate compiles, and cargo can't see that the
//! macro read their folder: without this, a migration added later is missing from a local build
//! until something else changes the crate (#280). `sqlx migrate build-script` writes the same.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
