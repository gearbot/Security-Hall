#![deny(warnings)]
#![deny(unsafe_code)]

use askama::Template;
use bincode;
use chrono::{Datelike, NaiveDate};
use lazy_static::lazy_static;
use serde::{Serialize, Deserialize};
use sled::Db;
use warp::{path, Filter, http::StatusCode};
use warp::{Rejection, reply::Response, Reply};

use log::info;
use flexi_logger::{Duplicate, Logger};

use std::{fmt, fs, error::Error};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::IpAddr;

#[derive(Debug, Deserialize)]
struct ServerConfig {
    ip: IpAddr,
    port: u16,
}

#[derive(Debug, Deserialize)]
pub struct AdminKey {
    username: String,
    key: String,
}

#[derive(Debug, Deserialize)]
struct Config {
    project_name: String,
    logging_dir: String,
    logging_level: String,
    server: ServerConfig,
    admin_keys: Option<Vec<AdminKey>>,
}

#[derive(Debug, Deserialize, Serialize, Hash)]
pub struct HallEntry {
    // This ID is randomly assigned and used for updates/deletions
    id: u64,
    anchor_key: Option<String>,
    // This ID is submitted by the user for linking to reports, incidents, etc. 
    reference_id: u64,
    affected_service: String,
    date: NaiveDate,
    summary: String,
    reporter: String,
    // This allows for a user to specify a handle, Twitter profile, etc to be displayed by their name.
    reporter_handle: Option<String>,
}

impl HallEntry {
    pub fn generate_anchor(&mut self) {
        let mut c_hasher = DefaultHasher::new();
        self.hash(&mut c_hasher);
        let hash = c_hasher.finish();
        
        // The anchors will end up similar to #2019-5B2CBFE78ED4BD69
        self.anchor_key = Some(format!("{}-{:X}", self.date.year(), hash))
    }
}

// This is the data that is needed in a POST to create a new record
#[derive(Debug, Deserialize, Serialize)]
pub struct RecordSubmission {
    // This ID is used for updating posts only. It is ignored elsewhere.
    id: Option<u64>,
    reference_id: u64,
    affected_service: String,
    // This is submitted in the form of Y-M-D
    date: Option<NaiveDate>,
    summary: String,
    reporter: String,
    reporter_handle: Option<String>,
}

#[derive(Debug, Serialize)]
struct OperationResponse {
    code: u16,
    message: String
}

#[derive(Copy, Clone, Debug)]
enum HallError {
    Failed,
    BadRequest,
}

// This exists only to handle unexpected errors due to bad user input
impl HallError {
    fn as_code(self) -> StatusCode {
        match self {
            HallError::Failed => StatusCode::INTERNAL_SERVER_ERROR,
            HallError::BadRequest => StatusCode::BAD_REQUEST
        }
    }

    fn as_u16(self) -> u16 {
        self.as_code().as_u16()
    }
}

impl Error for HallError {}

impl fmt::Display for HallError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            HallError::Failed => "The requested operation failed, please try again.",
            HallError::BadRequest => "Your request was malformed, please modify it and try again."
        })
    }
}

#[derive(Debug, Template)]
#[template(path = "report_list.html")]
struct ReportList<'a> {
    project_name: &'a str,
    reports: Vec<HallEntry>,
}

mod admin;
use admin::{
    add_record,
    remove_record,
    update_record
};


lazy_static! {
    static ref CONFIG: Config = {
        let config_string = fs::read_to_string("config.toml").expect("The config file couldn't be found!");
        toml::from_str(&config_string).expect("A value in your config is incorrect!")
    };
}

lazy_static! {
    static ref RECORD_DB: Db = {
        info!("Loading database...");
        Db::open("records").unwrap()
    };
}

fn main() {
    Logger::with_str(&CONFIG.logging_level)
        .log_to_file()
        .directory(&CONFIG.logging_dir)
        .duplicate_to_stderr(Duplicate::Info)
        .start()
        .unwrap();

    info!("Starting Hall of Fame...");
    info!("Project name set to: {}", &CONFIG.project_name);

    // Pre-initalize the database for about a ~500ms faster first load
    RECORD_DB.get("_").unwrap();

    let main_page = warp::path::end().map(||
        warp::reply::html(generate_record_page(&RECORD_DB, &CONFIG))
    );

    let static_content = path!("static").and(warp::fs::dir("static"));


    let get_key = warp::header::optional::<String>("Authorization");

    let record_listings = path!("list")
        .and(get_key)
        .map(|auth_key: Option<String>|
            match check_admin_permissions(&CONFIG, auth_key) {
                Ok(_) => warp::reply::json(&list_records(&RECORD_DB)).into_response(),
                Err(e) => e
            }
        );

    let record_add = path!("add")
        .and(warp::body::json().and(get_key))
        .map(|new_record: RecordSubmission, auth_key: Option<String>|
            match check_admin_permissions(&CONFIG, auth_key) {
                Ok(user) => add_record(new_record, user, &RECORD_DB),
                Err(e) => e
            }
        );

    
    let record_remove = path!("remove")
        .and(warp::path::param().and(get_key))
        .map(|id: u64, auth_key: Option<String>|
            match check_admin_permissions(&CONFIG, auth_key) {
                Ok(user) => remove_record(id, user, &RECORD_DB),
                Err(e) => e
            }
        );

    let record_update = path!("update")
        .and(warp::body::json().and(get_key))
        .map(|updated_record: RecordSubmission, auth_key: Option<String>|
            match check_admin_permissions(&CONFIG, auth_key) {
                Ok(user) => update_record(updated_record, user, &RECORD_DB),
                Err(e) => e
            }
        );


    let admin_get_interface = path!("admin").and(record_listings);
    let admin_post_interface = path!("admin").and(record_add.or(record_update).or(record_remove))
        .recover(handle_errors);

    let get_routes = warp::get2().and(main_page.or(admin_get_interface))
        .recover(handle_errors);
    
    warp::serve(get_routes.or(static_content).or(admin_post_interface)).run((CONFIG.server.ip, CONFIG.server.port))
}

fn check_admin_permissions(config: &Config, auth_key: Option<String>) -> Result<&AdminKey, Response>  {
    if let Some(keys) = &config.admin_keys {
        let bad_key_resp = generate_response("Invalid key", StatusCode::FORBIDDEN);
        match auth_key {
            Some(unchecked_key) => {
                match keys.iter().find(|key| key.key == unchecked_key) {
                    Some(valid_key) => Ok(valid_key),
                    None => Err(bad_key_resp)
                }
            }
            None => Err(bad_key_resp)
        }
    } else {
        let err_msg = "The admin interface is currently disabled";
        Err(generate_response(err_msg, StatusCode::FORBIDDEN))
    }
}

fn generate_record_page(db: &Db, config: &Config) -> String {
    let record_list = ReportList { project_name: &config.project_name, reports: list_records(db) };
    record_list.render().unwrap()
}

pub fn list_records(record_db: &Db) -> Vec<HallEntry> {
    let mut decoded_records: Vec<HallEntry> = Vec::with_capacity(10);

    let all_records = record_db.scan_prefix("SI-");
    for report in all_records.values() {
        decoded_records.push(bincode::deserialize(&report.unwrap()).unwrap())
    }

    decoded_records
}

fn generate_response(resp_message: &str, status_code: StatusCode) -> Response {
    let response = warp::reply::json(&OperationResponse {
        code: status_code.as_u16(),
        message: resp_message.to_string()
    });

    warp::reply::with_status(response, status_code).into_response()
}

// Any errors that are not user generated should become just a generic error
fn handle_errors(err: warp::Rejection) -> Result<impl Reply, Rejection> {
    match err.status() {
        StatusCode::INTERNAL_SERVER_ERROR => {
            let error = HallError::Failed;
            let resp_json = warp::reply::json(&OperationResponse {
                code: error.as_u16(),
                message: error.to_string()
            });

            Ok(warp::reply::with_status(resp_json, error.as_code()))
        }
        StatusCode::BAD_REQUEST => {
            let error = HallError::BadRequest;
            let resp_json = warp::reply::json(&OperationResponse {
                code: error.as_u16(),
                message: error.to_string()
            });

           Ok(warp::reply::with_status(resp_json, error.as_code()))
        }
        _ => Err(err)
    }
}
