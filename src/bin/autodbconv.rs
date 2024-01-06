fn main() {
    env_logger::init();
    match autodbconv::parse_ldf("tests/ldf/LIN_2.2A.ldf") {
        Ok(db) => {
            dbg!(db);
        }
        Err(e) => {
            dbg!(e);
        }
    }
}
