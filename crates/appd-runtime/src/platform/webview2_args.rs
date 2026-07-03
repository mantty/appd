const SPKI_LIST_FLAG: &str = "--ignore-certificate-errors-spki-list=";

pub(super) fn append_spki_pin(existing_args: Option<&str>, pin: &str) -> String {
    let flag = format!("{SPKI_LIST_FLAG}{pin}");
    let Some(existing_args) = existing_args.map(str::trim).filter(|args| !args.is_empty()) else {
        return flag;
    };

    let mut updated = false;
    let mut args = existing_args
        .split_ascii_whitespace()
        .map(|arg| append_pin_to_arg(arg, pin, &mut updated))
        .collect::<Vec<_>>();

    if !updated {
        args.push(flag);
    }
    args.join(" ")
}

fn append_pin_to_arg(arg: &str, pin: &str, updated: &mut bool) -> String {
    let Some(existing_pins) = arg.strip_prefix(SPKI_LIST_FLAG) else {
        return arg.to_owned();
    };

    *updated = true;
    if existing_pins
        .split(',')
        .any(|existing_pin| existing_pin == pin)
    {
        return arg.to_owned();
    }
    if existing_pins.is_empty() {
        return format!("{SPKI_LIST_FLAG}{pin}");
    }
    format!("{arg},{pin}")
}

#[cfg(test)]
mod tests {
    use super::append_spki_pin;

    #[test]
    fn creates_spki_arg_when_no_args_exist() {
        assert_eq!(
            append_spki_pin(None, "pin-a"),
            "--ignore-certificate-errors-spki-list=pin-a"
        );
    }

    #[test]
    fn appends_spki_arg_to_existing_args() {
        assert_eq!(
            append_spki_pin(Some("--disable-features=Foo"), "pin-a"),
            "--disable-features=Foo --ignore-certificate-errors-spki-list=pin-a"
        );
    }

    #[test]
    fn merges_pin_into_existing_spki_arg() {
        assert_eq!(
            append_spki_pin(
                Some("--foo --ignore-certificate-errors-spki-list=pin-a,pin-b"),
                "pin-c",
            ),
            "--foo --ignore-certificate-errors-spki-list=pin-a,pin-b,pin-c"
        );
    }

    #[test]
    fn leaves_existing_pin_unchanged() {
        assert_eq!(
            append_spki_pin(
                Some("--ignore-certificate-errors-spki-list=pin-a,pin-b"),
                "pin-b",
            ),
            "--ignore-certificate-errors-spki-list=pin-a,pin-b"
        );
    }
}
