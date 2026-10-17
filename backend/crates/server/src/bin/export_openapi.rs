//! Prints the OpenAPI document as pretty JSON. Touches neither the DB nor the environment.

fn main() {
    let json = serde_json::to_string_pretty(&api::openapi()).expect("serialize OpenAPI");
    println!("{json}");
}
