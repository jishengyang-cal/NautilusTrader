// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use smallvec::SmallVec;

use super::mstr::{MStr, Pattern, Topic};

/// Match a topic and a string pattern using iterative backtracking algorithm
/// pattern can contains -
/// '*' - match 0 or more characters after this
/// '?' - match any character once
/// 'a-z' - match the specific character
pub fn is_matching_backtracking(topic: MStr<Topic>, pattern: MStr<Pattern>) -> bool {
    let topic_bytes = topic.as_bytes();
    let pattern_bytes = pattern.as_bytes();

    is_matching(topic_bytes, pattern_bytes)
}

#[must_use]
pub fn is_matching(topic: &[u8], pattern: &[u8]) -> bool {
    // Stack to store states for backtracking (topic_idx, pattern_idx)
    // SmallVec avoids heap allocation for patterns with ≤16 wildcards
    let mut stack: SmallVec<[(usize, usize); 16]> = SmallVec::new();
    stack.push((0, 0));

    while let Some((mut i, mut j)) = stack.pop() {
        loop {
            // Found a match if we've consumed both strings
            if i == topic.len() && j == pattern.len() {
                return true;
            }

            // If we've reached the end of the pattern, break to try other paths
            if j == pattern.len() {
                break;
            }

            // Handle '*' wildcard
            if pattern[j] == b'*' {
                // Try skipping '*' entirely first
                stack.push((i, j + 1));

                // Continue with matching current character and keeping '*'
                if i < topic.len() {
                    i += 1;
                    continue;
                }
                break;
            }
            // Handle '?' or exact character match
            else if i < topic.len() && (pattern[j] == b'?' || topic[i] == pattern[j]) {
                // Continue matching linearly without stack operations
                i += 1;
                j += 1;
                continue;
            }

            // No match found in current path
            break;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use rand::{Rng, SeedableRng, rngs::StdRng};
    use regex::Regex;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("a", "*", true)]
    #[case("a", "a", true)]
    #[case("a", "b", false)]
    #[case("data.quotes.BINANCE", "data.*", true)]
    #[case("data.quotes.BINANCE", "data.quotes*", true)]
    #[case("data.quotes.BINANCE", "data.*.BINANCE", true)]
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.*", true)]
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ETH*", true)]
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ETH???", false)]
    #[case("data.trades.BINANCE.ETHUSD", "data.*.BINANCE.ETH???", true)]
    // We don't support [seq] style pattern
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[HC]USDT", false)]
    // We don't support [!seq] style pattern
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[!ABC]USDT", false)]
    // We don't support [^seq] style pattern
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[^ABC]USDT", false)]
    fn test_is_matching(#[case] topic: &str, #[case] pattern: &str, #[case] expected: bool) {
        assert_eq!(
            is_matching_backtracking(topic.into(), pattern.into()),
            expected
        );
    }

    fn convert_pattern_to_regex(pattern: &str) -> String {
        let mut regex = String::new();
        regex.push('^');

        for c in pattern.chars() {
            match c {
                '.' => regex.push_str("\\."),
                '*' => regex.push_str(".*"),
                '?' => regex.push('.'),
                _ => regex.push(c),
            }
        }

        regex.push('$');
        regex
    }

    #[rstest]
    #[case("a??.quo*es.?I?AN*ET?US*T", "^a..\\.quo.*es\\..I.AN.*ET.US.*T$")]
    #[case("da?*.?u*?s??*NC**ETH?", "^da..*\\..u.*.s...*NC.*.*ETH.$")]
    fn test_convert_pattern_to_regex(#[case] pat: &str, #[case] regex: &str) {
        assert_eq!(convert_pattern_to_regex(pat), regex);
    }

    fn generate_pattern_from_topic(topic: &str, rng: &mut StdRng) -> String {
        let mut pattern = String::new();

        for c in topic.chars() {
            let val: f64 = rng.random();
            // 10% chance of wildcard
            if val < 0.1 {
                pattern.push('*');
            }
            // 20% chance of question mark
            else if val < 0.3 {
                pattern.push('?');
            }
            // 20% chance of skipping
            else if val < 0.5 {
                continue;
            }
            // 50% chance of keeping the character
            else {
                pattern.push(c);
            };
        }

        pattern
    }

    #[rstest]
    fn test_matching_backtracking() {
        let topic = "data.quotes.BINANCE.ETHUSDT";
        let mut rng = StdRng::seed_from_u64(42);

        for i in 0..1000 {
            let pattern = generate_pattern_from_topic(topic, &mut rng);
            let regex_pattern = convert_pattern_to_regex(&pattern);
            let regex = Regex::new(&regex_pattern).unwrap();
            assert_eq!(
                is_matching_backtracking(topic.into(), pattern.as_str().into()),
                regex.is_match(topic),
                "Failed to match on iteration: {i}, pattern: \"{pattern}\", topic: {topic}, regex: \"{regex_pattern}\""
            );
        }
    }
}
