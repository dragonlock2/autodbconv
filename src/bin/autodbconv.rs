fn main() {
    env_logger::init();
    match autodbconv::parse_ldf("tests/ldf/LIN_2.2A.ldf") {
        Ok(_db) => {
            println!("TODO");
        }
        Err(e) => {
            dbg!(e);
        }
    }
}
