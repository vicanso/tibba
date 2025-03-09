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

use nanoid::nanoid;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::{NoContext, Timestamp, Uuid};

pub fn uuid() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ts = Timestamp::from_unix(NoContext, d.as_secs(), d.subsec_nanos());
    Uuid::new_v7(ts).to_string()
}

pub fn nanoid(size: usize) -> String {
    nanoid!(size)
}

pub fn float_to_fixed(value: f64, precision: usize) -> String {
    match precision {
        0 => format!("{:.0}", value),
        1 => format!("{:.1}", value),
        2 => format!("{:.2}", value),
        3 => format!("{:.3}", value),
        _ => format!("{:.4}", value),
    }
}

#[cfg(test)]
mod tests {
    use super::float_to_fixed;
    use pretty_assertions::assert_eq;
    #[test]
    fn to_fixed() {
        assert_eq!("1", float_to_fixed(1.123412, 0));
        assert_eq!("1.1", float_to_fixed(1.123412, 1));
        assert_eq!("1.12", float_to_fixed(1.123412, 2));
        assert_eq!("1.123", float_to_fixed(1.123412, 3));
        assert_eq!("1.1234", float_to_fixed(1.123412, 4));
    }
}
