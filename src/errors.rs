use log::debug;
use serde_json::json;

use crate::model::Holdstate;

pub fn missing_parameter_error(param: &str) -> serde_json::Value {
    json!({
        "code": -32602,
        "message": format!("missing required parameter: {}", param)
    })
}

pub fn invalid_argument_error(arg: &str) -> serde_json::Value {
    json!({
        "code": -1,
        "message": format!("Invalid argument: '{}'", arg)
    })
}

pub fn invalid_input_error(input: &str) -> serde_json::Value {
    json!({
        "code": -1,
        "message": format!("Invalid input: '{}'", input)
    })
}

pub fn invalid_hash_error(name: &str, token: &str) -> serde_json::Value {
    json!({
        "code": -32602,
        "message": format!("{}: should be a 32 byte hex value: \
        invalid token '{}'", name, token)
    })
}

pub fn payment_hash_missing_error(pay_hash: &str) -> serde_json::Value {
    json!({
        "code": -32602,
        "message": format!("payment_hash '{}' not found", pay_hash)
    })
}

pub fn invalid_integer_error(name: &str, integer: &str) -> serde_json::Value {
    json!({
        "code": -32602,
        "message": format!("{}: should be an unsigned 64 bit integer: \
        invalid token '{}'", name,integer)
    })
}

pub fn too_many_params_error(actual: usize, expected: usize) -> serde_json::Value {
    json!({
       "code": -32602,
       "message": format!("too many parameters: got {}, expected {}", actual, expected)
    })
}

pub fn wrong_hold_state_error(holdstate: Holdstate) -> serde_json::Value {
    debug!("Holdinvoice is in wrong state: '{}'", holdstate);
    json!({
        "code": -32602,
        "message": format!("Holdinvoice is in wrong state: '{}'", holdstate)
    })
}

pub fn config_value_error(name: &str, value: i64) -> String {
    format!("'{}' is invalid for {}", value, name)
}
