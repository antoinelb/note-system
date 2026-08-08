//! The v1 phase-1 invariant, enforced: canvas positions survive a full index
//! rebuild — including the discard-and-recreate path — because they live in
//! their own file beside the database, never inside it
//! (adr/2026-07-positions-separate-file.md).

use note_system::index::{Index, scan_vault};
use note_system::positions::Positions;

#[test]
fn positions_survive_a_full_index_rebuild() {
    let vault = tempfile::tempdir().expect("tempdir");
    for name in [
        ".index",
        "templates",
        "permanent",
        "time",
        "capture",
        "generated",
    ] {
        std::fs::create_dir(vault.path().join(name))
            .expect("create vault dir");
    }
    std::fs::write(
        vault.path().join("permanent/deep-modules.typ"),
        "#meta(id: \"deep-modules\", type: \"concept\", tags: ())\nBody.\n",
    )
    .expect("write note");

    let positions_path = vault.path().join(".index/positions");
    let mut positions = Positions::load(&positions_path);
    positions.set("deep-modules", 340.0, -120.5);
    positions.save().expect("save positions");

    // an ordinary rebuild: every table wiped and refilled from the files
    let db_path = vault.path().join(".index/index.db");
    let mut index = Index::open(&db_path).expect("open index");
    index
        .rebuild(&scan_vault(vault.path()).expect("scan vault"))
        .expect("rebuild index");

    // the harsher path: a corrupt database is discarded and recreated
    // (adr/2026-07-disposable-index-user-version.md), the exact gesture that
    // must not be able to reach user data
    drop(index);
    std::fs::write(&db_path, b"not a database").expect("corrupt db");
    let mut index = Index::open(&db_path).expect("recover index");
    index
        .rebuild(&scan_vault(vault.path()).expect("rescan vault"))
        .expect("rebuild recovered index");

    let survived = Positions::load(&positions_path);
    assert_eq!(survived.get("deep-modules"), Some((340.0, -120.5)));
}
