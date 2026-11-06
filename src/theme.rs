//! A small, lossless reader for GRUB `theme.txt`.
//!
//! It builds a tree of components and remembers where every value sits in the original text,
//! so that only the values that change are rewritten: comments, spacing and everything else
//! stay byte for byte the same.

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Plus,
    LBrace,
    RBrace,
    Eq,
    Colon,
    Word,
    Str,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
pub struct Prop {
    pub name: String,
    /// The value exactly as written, including quotes.
    pub raw: String,
    pub start: usize,
    pub end: usize,
}

impl Prop {
    pub fn quoted(&self) -> bool {
        self.raw.starts_with('"')
    }

    pub fn value(&self) -> &str {
        self.raw.trim_matches('"')
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub kind: String,
    pub props: Vec<Prop>,
    pub children: Vec<Block>,
}

impl Block {
    pub fn prop(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }

    pub fn id(&self) -> Option<&str> {
        self.prop("id").map(|p| p.value())
    }
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub src: String,
    pub globals: Vec<Prop>,
    pub blocks: Vec<Block>,
}

impl Theme {
    pub fn parse(src: &str) -> Result<Theme, String> {
        let tokens = lex(src)?;
        let mut p = Parser { src, tokens, pos: 0 };
        let (globals, blocks) = p.body(0)?;
        Ok(Theme { src: src.to_string(), globals, blocks })
    }

    pub fn global(&self, name: &str) -> Option<&Prop> {
        self.globals.iter().find(|p| p.name == name)
    }

    pub fn line_of(&self, offset: usize) -> usize {
        1 + self.src[..offset.min(self.src.len())].bytes().filter(|&b| b == b'\n').count()
    }
}

fn is_delim(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'}' | b'=' | b':' | b'"')
}

fn line_of_pos(src: &str, i: usize) -> usize {
    1 + src[..i].bytes().filter(|&b| b == b'\n').count()
}

fn lex(src: &str) -> Result<Vec<Token>, String> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line_start = true;
    while i < b.len() {
        let c = b[i];
        match c {
            b'\n' => {
                line_start = true;
                i += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' => {
                i += 1;
                continue;
            }
            b'#' if line_start => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i >= b.len() {
                    return Err(format!("line {}: a quote is never closed", line_of_pos(src, start)));
                }
                i += 1;
                out.push(Token { kind: Kind::Str, start, end: i });
            }
            b'{' => {
                out.push(Token { kind: Kind::LBrace, start: i, end: i + 1 });
                i += 1;
            }
            b'}' => {
                out.push(Token { kind: Kind::RBrace, start: i, end: i + 1 });
                i += 1;
            }
            b'=' => {
                out.push(Token { kind: Kind::Eq, start: i, end: i + 1 });
                i += 1;
            }
            b':' => {
                out.push(Token { kind: Kind::Colon, start: i, end: i + 1 });
                i += 1;
            }
            b'+' => {
                out.push(Token { kind: Kind::Plus, start: i, end: i + 1 });
                i += 1;
            }
            b'(' => {
                let start = i;
                while i < b.len() && b[i] != b')' {
                    i += 1;
                }
                if i >= b.len() {
                    return Err(format!("line {}: a ( is never closed", line_of_pos(src, start)));
                }
                i += 1;
                out.push(Token { kind: Kind::Word, start, end: i });
            }
            _ => {
                let start = i;
                while i < b.len() && !is_delim(b[i]) {
                    i += 1;
                }
                out.push(Token { kind: Kind::Word, start, end: i });
            }
        }
        line_start = false;
    }
    Ok(out)
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn text(&self, t: &Token) -> &'a str {
        &self.src[t.start..t.end]
    }

    fn line(&self, t: &Token) -> usize {
        line_of_pos(self.src, t.start)
    }

    fn body(&mut self, depth: usize) -> Result<(Vec<Prop>, Vec<Block>), String> {
        let mut props = Vec::new();
        let mut blocks = Vec::new();
        loop {
            let Some(tok) = self.peek().cloned() else {
                if depth == 0 {
                    return Ok((props, blocks));
                }
                return Err("the file ends before a { is closed".to_string());
            };
            match tok.kind {
                Kind::RBrace => {
                    if depth == 0 {
                        return Err(format!("line {}: a }} has no matching {{", self.line(&tok)));
                    }
                    self.pos += 1;
                    return Ok((props, blocks));
                }
                Kind::Plus => {
                    self.pos += 1;
                    let name = self.peek().cloned().ok_or("the file ends after a +")?;
                    if name.kind != Kind::Word {
                        return Err(format!("line {}: expected a component name after +", self.line(&name)));
                    }
                    self.pos += 1;
                    let brace = self.peek().cloned().ok_or("the file ends before a {")?;
                    if brace.kind != Kind::LBrace {
                        return Err(format!("line {}: expected {{ after the component name", self.line(&brace)));
                    }
                    self.pos += 1;
                    let kind = self.text(&name).to_string();
                    let (p, c) = self.body(depth + 1)?;
                    blocks.push(Block { kind, props: p, children: c });
                }
                Kind::Word => {
                    self.pos += 1;
                    let sep = self.peek().cloned().ok_or_else(|| format!("line {}: a property has no value", self.line(&tok)))?;
                    if sep.kind != Kind::Eq && sep.kind != Kind::Colon {
                        return Err(format!("line {}: expected = or : after `{}`", self.line(&sep), self.text(&tok)));
                    }
                    self.pos += 1;
                    let val = self.peek().cloned().ok_or_else(|| format!("line {}: `{}` has no value", self.line(&tok), self.text(&tok)))?;
                    if val.kind != Kind::Word && val.kind != Kind::Str {
                        return Err(format!("line {}: `{}` has no value", self.line(&tok), self.text(&tok)));
                    }
                    self.pos += 1;
                    props.push(Prop {
                        name: self.text(&tok).to_string(),
                        raw: self.text(&val).to_string(),
                        start: val.start,
                        end: val.end,
                    });
                }
                _ => {
                    return Err(format!("line {}: unexpected `{}`", self.line(&tok), self.text(&tok)));
                }
            }
        }
    }
}

#[cfg(test)]
pub mod tests_sample {
    pub const NEXUS: &str = r##"# GRUB2 Theme - Caelestia Nexus
title-text: ""
desktop-image: "background.png"
terminal-font: "Roboto Mono Regular 22"
terminal-left: "20%"
terminal-top: "20%"
terminal-width: "60%"
terminal-height: "60%"
terminal-border: "0"

+ boot_menu {
  left = 380
  top = 230
  width = 1160
  height = 620
  item_font = "Google Sans Flex Regular 22"
  item_color = "#72757e"
  selected_item_color = "#e3e5ef"
  item_height = 48
  item_padding = 4
  item_spacing = 16
  menu_pixmap_style = ""
  item_pixmap_style = "item_*.png"
}

+ progress_bar {
  id = "__timeout__"
  left = 380
  top = 960
  width = 1160
  height = 12
  bar_style = "timeout_bg_*.png"
}
"##;
}

#[cfg(test)]
mod tests {
    use super::tests_sample::NEXUS;
    use super::*;

    #[test]
    fn reads_globals_blocks_and_values() {
        let t = Theme::parse(NEXUS).unwrap();
        assert_eq!(t.globals.len(), 8);
        assert_eq!(t.global("desktop-image").unwrap().value(), "background.png");
        assert_eq!(t.blocks.len(), 2);
        assert_eq!(t.blocks[0].kind, "boot_menu");
        assert_eq!(t.blocks[0].prop("left").unwrap().raw, "380");
        assert_eq!(t.blocks[1].id(), Some("__timeout__"));
    }

    #[test]
    fn spans_point_at_the_value_only() {
        let t = Theme::parse(NEXUS).unwrap();
        let p = t.blocks[0].prop("width").unwrap();
        assert_eq!(&t.src[p.start..p.end], "1160");
    }

    #[test]
    fn colours_are_not_comments_and_comments_are_skipped() {
        let t = Theme::parse("# c\n+ label { color = #88F text = \"hi # there\" }\n").unwrap();
        assert_eq!(t.blocks[0].prop("color").unwrap().raw, "#88F");
        assert_eq!(t.blocks[0].prop("text").unwrap().value(), "hi # there");
    }

    #[test]
    fn nested_blocks_and_single_line_blocks() {
        let t = Theme::parse("+ hbox { left = 10\n + label { text = \"a\" width = 5 }\n}").unwrap();
        assert_eq!(t.blocks[0].children.len(), 1);
        assert_eq!(t.blocks[0].children[0].prop("width").unwrap().raw, "5");
    }

    #[test]
    fn reports_broken_files_with_a_line_number() {
        assert!(Theme::parse("+ boot_menu {\n left = 1\n").unwrap_err().contains("closed"));
        assert!(Theme::parse("}\n").unwrap_err().contains("line 1"));
        assert!(Theme::parse("a: \"oops\n").unwrap_err().contains("line 1"));
        assert!(Theme::parse("+ x {\n left =\n}").is_err());
    }

    #[test]
    fn line_numbers() {
        let t = Theme::parse(NEXUS).unwrap();
        let p = t.blocks[0].prop("left").unwrap();
        assert_eq!(t.line_of(p.start), 12);
    }
}
