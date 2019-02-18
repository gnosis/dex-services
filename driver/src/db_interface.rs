
pub mod db_interface{    
    use mongodb::bson;
    use mongodb::db::ThreadedDatabase;
    use mongodb::{Client, ThreadedClient};

    use std::env;

    pub struct DbInterface{
        pub client: Client,
        pub db_host: String,
        pub db_port: String,
    }

    impl DbInterface {
        pub fn new(args: &[String]) -> Result<DbInterface, &'static str> {
            let db_host = env::var("DB_HOST").unwrap();
            let db_port = env::var("DB_PORT").unwrap();
            let client = Client::connect(&db_host, db_port.parse::<u16>().unwrap())
                .expect("Failed to initialize standalone client");

            Ok(DbInterface { client, db_host, db_port })
        }
}
}