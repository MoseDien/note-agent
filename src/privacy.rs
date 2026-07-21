use regex::{Captures, Regex};
use std::sync::LazyLock;

static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").unwrap());
static PHONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:\+?86[- ]?)?1[3-9]\d{9}").unwrap());
static ID_CARD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b\d{17}[0-9X]\b").unwrap());
static BANK_CARD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:\d[ -]?){15,18}\d\b").unwrap());
static IPV4: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap());

pub fn redact(input: &str) -> String {
    let rules = [
        (&*EMAIL, "[邮箱]"),
        (&*PHONE, "[手机号]"),
        (&*ID_CARD, "[身份证号]"),
        (&*BANK_CARD, "[银行卡号]"),
        (&*IPV4, "[IP地址]"),
    ];
    rules
        .into_iter()
        .fold(input.to_owned(), |text, (re, replacement)| {
            re.replace_all(&text, |_: &Captures| replacement)
                .into_owned()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_common_pii() {
        let output = redact("电话 13812345678，邮箱 a@example.com，IP 10.0.0.1");
        assert!(!output.contains("13812345678"));
        assert!(!output.contains("a@example.com"));
        assert!(!output.contains("10.0.0.1"));
    }
}
