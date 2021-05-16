use std::cmp::Ordering;
use std::fmt::Write;
use std::collections::HashSet;

#[derive(Debug)]
enum WriterItem{
    StaticItem(u32, String),
    TaggedGroup(Vec<(String, Writer, bool)>),
    Break
}

#[derive(Debug)]
pub(crate) struct Writer {
    file: Option<String>,
    pub(crate) line: u32,
    items: Vec<WriterItem>,
}

#[derive(Debug)]
pub struct TaggedGroupHandle<'a> {
    owner: &'a mut Writer,
    tagged_group_idx: usize
}


const INDENT_WIDTH: usize = 2;

impl Writer {
    pub(crate) fn new(file: &Option<String>, line: u32) -> Self {
        Self {
            file: file.clone(),
            line,
            items: Vec::new(),
        }
    }

    pub(crate) fn add_fixed_item(&mut self, item: String, position: u32) {
        self.items.push(WriterItem::StaticItem(position, item));
    }

    pub(crate) fn add_break_if_new_item(&mut self, position: u32) {
        if position == 0 || position == u32::MAX {
            self.items.push(WriterItem::Break);
        }
    }

    pub(crate) fn add_tagged_group(&mut self, size: usize) -> TaggedGroupHandle {
        self.items.push(WriterItem::TaggedGroup(Vec::with_capacity(size)));
        let idx = self.items.len() - 1;
        TaggedGroupHandle {
            owner: self,
            tagged_group_idx: idx
        }
    }


    // finish()
    // consumes self to build a String of the file
    pub(crate) fn finish(self) -> String {
        let (_, text) = self.finish_internal(0);

        // there is an extra space at the beginning of the output - in all other blocks
        // a separator between the tag and the content is needed. It is easier to remove
        // this space here than to add a special case to prevent it from being generated
        text[1..].to_owned()
    }


    // finish_internal()
    // recursively construct each indentation level of the file
    fn finish_internal(self, indent: usize) -> (u32, String) {
        let mut outstring = "".to_string();
        let mut current_line = self.line;
        let mut empty_block = true;
        let mut should_break = false;
        for item in self.items {
            match item {
                WriterItem::StaticItem(item_line, text) => {
                    // add the text of static items, while inserting linebreaks with indentation as needed
                    outstring.write_str(&make_whitespace(current_line, item_line, indent, should_break, false)).unwrap();
                    outstring.write_str(&text).unwrap();
                    should_break = false;
                    current_line = item_line;
                }
                WriterItem::Break => {
                    should_break = true;
                }
                WriterItem::TaggedGroup(mut group) => {
                    // sort the items in this group according to the sorting function
                    group.sort_by(Self::sort_function);
                    // build the text containing all of the group items and append it to the string for this block
                    let (last_line, text) = Self::finish_group(current_line, group, indent, empty_block);
                    outstring.write_str(&text).unwrap();
                    current_line = last_line;
                }
            }
            empty_block = false;
        }

        (current_line, outstring)
    }


    fn finish_group(start_line: u32, group: Vec<(String, Writer, bool)>, indent: usize, empty_block: bool) -> (u32, String) {
        let mut outstring = "".to_string();
        let mut current_line = start_line;
        let mut included_files = HashSet::<String>::new();
        let grouplen = group.len();
        for (tag, item, is_block) in group {
            // check if the element should be written to this file, or if an /include statement should be generated instead
            if item.file.is_none() {
                // if needed add whitespace
                outstring.write_str(&make_whitespace(current_line, item.line, indent, true, is_block)).unwrap();

                // if the opening block tag shares the line with the previous opening block tag, then (probably) no additional indentation is needed
                // this is a hacky heuristic that fixes the indentation of IF_DATA blocks
                let newindent = if empty_block && current_line == item.line && grouplen == 1 {
                    indent
                } else {
                    indent + 1
                };
                // build the text for this block item
                let (endline, text) = item.finish_internal(newindent);
                current_line = endline;
                if is_block {
                    outstring.write_str("/begin ").unwrap();
                }
                outstring.write_str(&tag).unwrap();
                outstring.write_str(&text).unwrap();
                if is_block {
                    outstring.write_str(&make_whitespace(current_line, current_line + 1, indent, false, false)).unwrap();
                    outstring.write_str("/end ").unwrap();
                    outstring.write_str(&tag).unwrap();
                    current_line += 1;
                }
            } else {
                // this element came from an include file
                let incfile = item.file.as_ref().unwrap();
                // check if the /include has been added yet and add it if needed
                if included_files.get(incfile).is_none() {
                    outstring.write_str(&make_whitespace(current_line, current_line + 1, indent, true, true)).unwrap();
                    outstring.write_str("/include \"").unwrap();
                    outstring.write_str(&incfile).unwrap();
                    outstring.write_str("\"").unwrap();
                    current_line += 1;

                    included_files.insert(incfile.to_owned());
                }
            }
        }
        (current_line, outstring)
    }


    fn sort_function(a: &(String, Writer, bool), b: &(String, Writer, bool)) -> Ordering {
        let (tag_a, item_a, _) = a;
        let (tag_b, item_b, _) = b;

        // hard-code a little bit of odering of blocks: at the top level, ASAP2_VERSION must be the first block
        // within MODULE, A2ML must be present before there are any IF_DATA blocks so that these can be decoded
        if tag_a == "ASAP2_VERSION" || tag_a == "A2ML" {
            Ordering::Less
        } else if tag_b == "ASAP2_VERSION" || tag_b == "A2ML" {
            Ordering::Greater
        } else {
            // handle included elements
            if item_a.file.is_some() && item_b.file.is_some() {
                // both items are included
                let incname_a = item_a.file.as_ref().unwrap();
                let incname_b = item_b.file.as_ref().unwrap();

                // sort included elements alphabetically by the name of the file they were included from
                incname_a.cmp(incname_b)
            } else if item_a.file.is_some() && item_b.file.is_none() {
                // a included, b is not: put a first to group all included items at the beginnning
                Ordering::Less
            } else if item_a.file.is_none() && item_b.file.is_some() {
                // a not included, b is included: put b first to group all included items at the beginnning
                Ordering::Greater
            } else {
                // no special cases basd on the tag or include status
                // items that have a line number (i. they were loaded from an input file) come first
                // items without a line number (created at runtime) are placed at the end
                if item_a.line != 0 && item_b.line != 0 {
                    item_a.line.cmp(&item_b.line)
                } else if item_a.line != 0 && item_b.line == 0 {
                    Ordering::Less
                } else if item_a.line == 0 && item_b.line != 0 {
                    Ordering::Greater
                } else {
                    // neither item has a line number, sort them alphabetically by tag
                    tag_a.cmp(tag_b)
                }
            }
        }
    }
}


impl<'a> TaggedGroupHandle<'a> {
    pub(crate) fn add_tagged_item(&mut self, tag: &str, item: Writer, is_block: bool) {
        if let WriterItem::TaggedGroup(tagmap) = &mut self.owner.items[self.tagged_group_idx] {
            tagmap.push((tag.to_string(), item, is_block));
        }
    }
}


pub fn escape_string(value: &str) -> String {
    // escaping is an expensive operation, so check if anything needs to be done first
    if value.contains(|c| c == '\'' || c == '"' || c == '\\' || c == '\n' || c == '\t') {
        let input_chars: Vec<char> = value.chars().collect();
        let mut output_chars: Vec<char> = Vec::new();

        for c in input_chars {
            if c == '\'' || c == '"' || c == '\\' || c == '\n' || c == '\t' {
                output_chars.push('\\');
            }
            output_chars.push(c);
        }

        output_chars.iter().collect()
    } else {
        value.to_string()
    }
}


// make_whitespace()
// Create whitespace between two elements.
// For pre-existing elements, this depends on the line numbers of the elements:
// - equal line numbers, and both elements have line numbers: separate with a space
// - line number differs by one: add a newline and indent
// - if line numbers differ by more than one and the new item is the start of a block element: add two newlines and indent
// For newly created elements there are blocks in whilch all line numbers are set to zero and keywords where all line numbers are equal to u32::MAX
// The flag break_new helps to improve the layout in these cases as it is set when formatting begins for a block or keyword
fn make_whitespace(current_line: u32, item_line: u32, indent: usize, break_new: bool, allow_empty_line: bool) -> &'static str {
    let must_break = break_new && current_line == item_line && current_line == u32::MAX;
    // generate indents by returning slices of base_str. The goal is to avoid the extra allocations of making Strings instead of &str.
    // This limits indents to the length of this string, which should not be a limit that matters in practice.
    let base_str: &'static str = // must contain 120 spaces
        "\n\n                                                                                                                        ";

    if current_line != 0 && current_line == item_line && !must_break {
        " "
    } else if current_line + 1 == item_line ||
              (current_line == 0 && item_line == 0) ||
              !allow_empty_line {
        if indent < 120 / INDENT_WIDTH {
            &base_str[1..(indent * INDENT_WIDTH + 2)]
        } else {
            &base_str[1..120+2]
        }
    } else {
        if indent < 120 / INDENT_WIDTH {
            &base_str[..(indent * INDENT_WIDTH + 2)]
        } else {
            &base_str[..120+2]
        }
    }
}


pub(crate) fn format_u8(val_format: (u8, bool)) -> String {
    let (value, is_hex) = val_format;
    if !is_hex {
        format!("{}", value)
    } else {
        format!("0x{:0X}", value)
    }
}

pub(crate) fn format_u16(val_format: (u16, bool)) -> String {
    let (value, is_hex) = val_format;
    if !is_hex {
        format!("{}", value)
    } else {
        format!("0x{:0X}", value)
    }
}

pub(crate) fn format_u32(val_format: (u32, bool)) -> String {
    let (value, is_hex) = val_format;
    if !is_hex {
        format!("{}", value)
    } else {
        format!("0x{:0X}", value)
    }
}

pub(crate) fn format_u64(val_format: (u64, bool)) -> String {
    let (value, is_hex) = val_format;
    if !is_hex {
        format!("{}", value)
    } else {
        format!("0x{:0X}", value)
    }
}


pub(crate) fn format_i8(val_format: (i8, bool)) -> String {
    let (value, is_hex) = val_format;
    if !is_hex {
        format!("{}", value)
    } else {
        format!("0x{:0X}", value)
    }
}

pub(crate) fn format_i16(val_format: (i16, bool)) -> String {
    let (value, is_hex) = val_format;
    if !is_hex {
        format!("{}", value)
    } else {
        format!("0x{:0X}", value)
    }
}

pub(crate) fn format_i32(val_format: (i32, bool)) -> String {
    let (value, is_hex) = val_format;
    if !is_hex {
        format!("{}", value)
    } else {
        format!("0x{:0X}", value)
    }
}

pub(crate) fn format_i64(val_format: (i64, bool)) -> String {
    let (value, is_hex) = val_format;
    if !is_hex {
        format!("{}", value)
    } else {
        format!("0x{:0X}", value)
    }
}


pub(crate) fn format_float(value: f32) -> String {
    if value == 0f32 {
        "0".to_string()
    } else if value < -1e+10 || (-0.0001 < value && value < 0.0001) || 1e+10 < value {
        format!("{:e}", value)
    } else {
        format!("{}", value)
    }
}


pub(crate) fn format_double(value: f64) -> String {
    if value == 0f64 {
        "0".to_string()
    } else if value < -1e+10 || (-0.0001 < value && value < 0.0001) || 1e+10 < value {
        format!("{:e}", value)
    } else {
        format!("{}", value)
    }
}
