/// A minimal JSON writer.
///
/// The crate hand-rolls every other encoding it uses rather than deriving one, and a report that
/// crosses into Kotlin and into whatever plots it afterwards is not a reason to take a
/// serialisation framework as a dependency for one direction of one type.
#[derive(Debug, Default)]
pub(crate) struct Json {
    out: String,
    /// True at the start of a container, where a separator would be wrong.
    fresh: bool,
}

impl Json {
    pub(crate) const fn new() -> Self {
        Self {
            out: String::new(),
            fresh: true,
        }
    }

    pub(crate) fn object(&mut self, fields: impl FnOnce(&mut Self)) {
        self.separate();
        self.out.push('{');
        self.fresh = true;
        fields(self);
        self.out.push('}');
        self.fresh = false;
    }

    pub(crate) fn array(&mut self, items: impl FnOnce(&mut Self)) {
        self.separate();
        self.out.push('[');
        self.fresh = true;
        items(self);
        self.out.push(']');
        self.fresh = false;
    }

    pub(crate) fn field(&mut self, name: &str, value: impl FnOnce(&mut Self)) {
        self.key(name);
        value(self);
    }

    pub(crate) fn string(&mut self, value: &str) {
        self.separate();
        self.out.push('"');
        for character in value.chars() {
            match character {
                '"' => self.out.push_str("\\\""),
                '\\' => self.out.push_str("\\\\"),
                '\n' => self.out.push_str("\\n"),
                '\r' => self.out.push_str("\\r"),
                '\t' => self.out.push_str("\\t"),
                control if control < ' ' => {
                    self.out.push_str(&format!("\\u{:04x}", control as u32));
                }
                other => self.out.push(other),
            }
        }
        self.out.push('"');
    }

    pub(crate) fn number(&mut self, value: u64) {
        self.separate();
        self.out.push_str(&value.to_string());
    }

    pub(crate) fn boolean(&mut self, value: bool) {
        self.separate();
        self.out.push_str(if value { "true" } else { "false" });
    }

    pub(crate) fn null(&mut self) {
        self.separate();
        self.out.push_str("null");
    }

    pub(crate) fn optional_number(&mut self, value: Option<u64>) {
        match value {
            Some(number) => self.number(number),
            None => self.null(),
        }
    }

    pub(crate) fn finish(self) -> String {
        self.out
    }

    fn key(&mut self, name: &str) {
        self.string(name);
        self.out.push(':');
        self.fresh = true;
    }

    fn separate(&mut self) {
        if !self.fresh {
            self.out.push(',');
        }
        self.fresh = false;
    }
}
