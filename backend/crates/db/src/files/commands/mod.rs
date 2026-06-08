//! Mutating commands for the file tree. Each runs in one transaction, sets
//! attribution (created_by/updated_by/deleted_by) from the caller, and
//! re-enforces the capacity/uniqueness invariants in-transaction for race
//! safety (the service pre-checks them too, for precise errors).

pub mod checks;
pub mod create;
pub mod delete;
pub mod move_node;
pub mod restore;
pub mod save;
pub mod update;
