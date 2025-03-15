// Copyright 2025 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::CODE_LISTEN_ADDR;
use super::is_disabled;
use std::borrow::Cow;
use std::net::ToSocketAddrs;
use validator::ValidationError;

type Result<T> = std::result::Result<T, ValidationError>;

pub fn x_listen_addr(addr: &str) -> Result<()> {
    if is_disabled(CODE_LISTEN_ADDR) {
        return Ok(());
    }
    // validate port
    if let Some(value) = addr.strip_prefix(':') {
        let port = value.parse::<u16>().map_err(|_| {
            ValidationError::new(CODE_LISTEN_ADDR)
                .with_message(Cow::from("Port must be a number between 1 and 65535"))
        })?;
        if port == 0 {
            return Err(
                ValidationError::new(CODE_LISTEN_ADDR).with_message(Cow::from("Port cannot be 0"))
            );
        }
        return Ok(());
    }

    // validate address to socket addrs
    let addr_result = addr.to_socket_addrs();
    match addr_result {
        Ok(mut addrs) => {
            // ensure at least one valid address
            if addrs.next().is_none() {
                return Err(ValidationError::new(CODE_LISTEN_ADDR)
                    .with_message(Cow::from("No valid address found")));
            }
        }
        Err(_) => {
            return Err(ValidationError::new(CODE_LISTEN_ADDR)
                .with_message(Cow::from("Invalid address format")));
        }
    }
    Ok(())
}
