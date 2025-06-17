use anyhow::anyhow;

use crate::model::Holdstate;

pub fn missing_parameter_error(param: &str) -> anyhow::Error {
    anyhow!("missing required parameter: {}", param)
}

pub fn invalid_argument_error(arg: &str) -> anyhow::Error {
    anyhow!("Invalid argument: `{}`", arg)
}

pub fn invalid_input_error(input: &str) -> anyhow::Error {
    anyhow!("Invalid input: `{}`", input)
}

pub fn invalid_scid_error(input: &str) -> anyhow::Error {
    anyhow!("Invalid short_channel_id: `{}`", input)
}

pub fn invalid_hash_error(name: &str, token: &str) -> anyhow::Error {
    anyhow!(
        "{}: should be a 32 byte hex value: invalid token `{}`",
        name,
        token
    )
}

pub fn payment_hash_missing_error(pay_hash: &str) -> anyhow::Error {
    anyhow!("payment_hash `{}` not found", pay_hash)
}

pub fn invalid_integer_error(name: &str, integer: &str) -> anyhow::Error {
    anyhow!(
        "{}: should be an unsigned 64 bit integer: invalid token `{}`",
        name,
        integer
    )
}

pub fn invalid_amount_error() -> anyhow::Error {
    anyhow!("amount_msat: should be positive msat")
}

pub fn too_many_params_error(actual: usize, expected: usize) -> anyhow::Error {
    anyhow!("too many parameters: got {}, expected {}", actual, expected)
}

pub fn wrong_hold_state_error(holdstate: Holdstate) -> anyhow::Error {
    log::debug!("Holdinvoice is in wrong state: `{}`", holdstate);
    anyhow!("Holdinvoice is in wrong state: `{}`", holdstate)
}

pub fn config_value_error(name: &str, value: i64) -> anyhow::Error {
    anyhow!("`{}` is invalid for {}", value, name)
}

pub fn internal_error(msg: &str) -> anyhow::Error {
    anyhow!("Internal error: {}", msg)
}
