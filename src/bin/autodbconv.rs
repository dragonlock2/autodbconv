fn main() {
    match autodbconv::parse_ldf("tests/ldf/LIN_2.2A.ldf") {
        Ok(db) => {
            println!("TODO");
        }
        Err(e) => {
            dbg!(e);
        }
    }
}
