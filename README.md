# Security Hall

## About
Ever needed a simple way to acknowledge reporters who have found issues in your software? This project does just that. It provides a clean web page that displays any security reports submitted via a easy to use REST API. The records are stored in a small on-disk database, powered by [sled](https://github.com/spacejam/sled) and the web side is handled by [warp](https://github.com/seanmonstar/warp).

By default, a basic CSS stylesheet is provided, but it can be configured for whatever needs or theming desired by the user.

## Admin Interface
To add, remove, or update reports inside the record store, the admin REST API is used. It can be located at `http://host/admin/` and it has 4 endpoints:
- `/list` - Lists all the current records in JSON form
- `/add` - Add the provided record in the request body to the database
- `/remove/{ID}` - Remove the corresponding record to the ID provided
- `/update` - Update a record with the provided ID and body. 

By default the interface is disabled, but can easily be enabled by uncommenting the bottom section of the config file. All requests to the API must include an `application/json` header and then a `Authorization` header that contains a key registered in the config. To see the structure of record addition/updating, see below (Any values with `Option<>` around them aren't required):

```json
{
    // This ID is used for updating posts only. It is ignored when adding a new report.
    "id": Option<92811>,

    // This is used purely for admin reference to arbitrary internal IDs, and isn't publically visible.
    "reference_id": 1,

    "affected_service": "Some System",

    // Submitted in the form of Y-M-D and is optional. The current date is used when not provided.
    "date": Option<"2019-8-24">,
    "summary": "An issue...",
    "reporter": "Somebody",
    "reporter_handle": Option<"@Maybe">,
}
```

## Config Layout
Explanations of what values do, and more detail on setting up the admin interface, are located in the default config.

## Building and Setup
Prerequisites: Have a Rust toolchain installed.

1. Clone the repository to a directory and `cd` to it.
2. Copy `default_config.toml` to `config.toml` and modify values as needed.
3. Run `cargo run` to build and start the project, or `cargo build --release` if you want a production binary.
4. Use the admin API for record manipulation


### License
This entire project falls under the MIT License and may be used as such