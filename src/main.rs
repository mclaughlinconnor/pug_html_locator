use tree_sitter::{Language, Node, Parser, TreeCursor};
extern "C" {
    fn tree_sitter_pug() -> Language;
}

struct Range {
    html_end: usize,
    html_start: usize,
    pug_end: usize,
    pug_start: usize,
}

struct State {
    html_text: String,
    pug_text: String,
    ranges: Vec<Range>,
}

fn is_void_element(tag_name: &str) -> bool {
    match tag_name {
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta"
        | "param" | "source" | "track" | "wbr" => {
            return true;
        }
        _ => {
            return false;
        }
    }
}

fn main() {
    let mut parser = Parser::new();

    let pug_input = r#"
        tag(attribute=isAuthenticated ? true : false, attribute)
          tag_two(attribute)
    "#;

    let language = unsafe { tree_sitter_pug() };
    parser.set_language(language).unwrap();

    let tree = parser.parse(pug_input, None).unwrap();
    let mut root_node = tree.root_node();

    let mut state = State {
        html_text: String::new(),
        pug_text: pug_input.to_string(),
        ranges: Vec::new(),
    };

    traverse_tree(&mut root_node, pug_input.as_bytes(), &mut state);

    println!("{}", pug_input);
    println!("{}\n", root_node.to_sexp());
    println!("{}", state.html_text);
    for range in state.ranges {
        println!(
            "'{}' => '{}'",
            state.html_text[range.html_start..range.html_end].to_string(),
            state.pug_text[range.pug_start..range.pug_end].to_string()
        );
    }
}

fn push_range(state: &mut State, to_push: &str, pug_range: Option<tree_sitter::Range>) {
    match pug_range {
        Some(range) => {
            let html_len = state.html_text.len();

            let range = Range {
                html_start: html_len,
                html_end: html_len + to_push.len(),
                pug_start: range.start_byte,
                pug_end: range.end_byte,
            };

            state.ranges.push(range);
        }
        _ => {}
    }

    state.html_text.push_str(&to_push);
}

fn visit_attributes(cursor: &mut TreeCursor, node: &mut Node, source: &[u8], state: &mut State) {
    let mut first = true;

    let mut child_cursor = cursor.clone();
    for attribute in node.named_children(&mut child_cursor) {
        if !first {
            push_range(state, ", ", None);
        } else {
            first = false;
        }

        let mut attribute_cursor = cursor.clone();
        let mut children = attribute.named_children(&mut attribute_cursor);

        let attribute_name = children.next().unwrap();
        let attribute_value = children.next();

        let name_text = attribute_name.utf8_text(source).unwrap();
        push_range(state, name_text, Some(attribute_name.range()));
        push_range(state, "=", None);

        match attribute_value {
            Some(attribute_value) => {
                let text = attribute_value.utf8_text(source).unwrap().to_string();

                match attribute_value.kind() {
                    // Just make javascript attributes into valid HTML
                    "javascript" => {
                        push_range_surround(state, &text, attribute_value.range(), "'");
                    }
                    "quoted_attribute_value" => {
                        push_range(state, &text, Some(attribute_value.range()));
                    }
                    _ => {}
                }
            }
            None => {
                push_range_surround(
                    state,
                    attribute_name.utf8_text(source).unwrap(),
                    attribute_name.range(),
                    "'",
                );
            }
        }
    }
}

fn push_range_surround(
    state: &mut State,
    to_push: &str,
    pug_range: tree_sitter::Range,
    surround: &str,
) {
    push_range(state, surround, None);
    push_range(state, to_push, Some(pug_range));
    push_range(state, surround, None);
}

fn visit_tag(cursor: &mut TreeCursor, node: &mut Node, source: &[u8], state: &mut State) {
    let mut cursor_mutable = cursor.clone();

    let mut child_nodes = node.named_children(&mut cursor_mutable);
    let name_node = child_nodes.next().unwrap();
    let name = name_node.utf8_text(source).unwrap();

    push_range(state, "<", None);
    push_range(state, name, Some(name_node.range()));

    let mut has_closed_open_tag = false;

    for mut child_node in child_nodes {
        if child_node.kind() == "attributes" {
            push_range(state, " ", None);
            traverse_tree(&mut child_node, source, state);
            continue;
        }

        if is_void_element(name) {
            push_range(state, "/>", None);
            break;
        }

        if !has_closed_open_tag {
            push_range(state, ">", None);
            has_closed_open_tag = true;
        }

        if child_node.kind() == "content" {
            traverse_tree(&mut child_node, source, state);
            continue;
        }

        if child_node.kind() == "children" {
            traverse_tree(&mut child_node, source, state);
            continue;
        }
    }

    if !has_closed_open_tag {
        push_range(state, ">", None);
    }

    if !is_void_element(name) {
        push_range(state, &format!("</{}>", name).to_string(), None);
    }

    // TODO: parse content for {{angular_interpolation}} using angular_content parser
}

fn visit_conditional(cursor: &mut TreeCursor, node: &mut Node, source: &[u8], state: &mut State) {
    let mut child_cursor = cursor.clone();
    let mut conditional_cursor = node.walk();

    conditional_cursor.goto_first_child();
    conditional_cursor.goto_next_sibling();

    if conditional_cursor.node().kind() == "javascript" {
        let condition = conditional_cursor.node();

        push_range(state, "<script>return ", None);
        push_range(
            state,
            condition.utf8_text(source).unwrap(),
            Some(condition.range()),
        );
        push_range(state, ";</script>", None);
        conditional_cursor.goto_next_sibling();
    }

    conditional_cursor.goto_next_sibling();

    let children = conditional_cursor.node().named_children(&mut child_cursor);
    for mut child in children {
        traverse_tree(&mut child, source, state);
    }
}

fn visit_pipe(cursor: &mut TreeCursor, _node: &mut Node, source: &[u8], state: &mut State) {
    cursor.goto_first_child();
    while cursor.goto_next_sibling() {
        if cursor.node().is_named() {
            traverse_tree(&mut cursor.node(), source, state);
        }
    }
}

fn visit_tag_interpolation(
    _cursor: &mut TreeCursor,
    node: &mut Node,
    source: &[u8],
    state: &mut State,
) {
    let mut interpolation_cursor = node.walk();

    interpolation_cursor.goto_first_child();
    interpolation_cursor.goto_next_sibling();
    let children = interpolation_cursor
        .node()
        .named_children(&mut interpolation_cursor);

    for mut child in children {
        traverse_tree(&mut child, source, state);
    }
}

fn traverse_tree(node: &mut Node, source: &[u8], state: &mut State) {
    let node_type = node.kind();

    let mut cursor = node.walk();

    if node.is_named() {
        match node_type {
            "source_file" | "children" => {
                let mut child_cursor = cursor.clone();
                let children = node.named_children(&mut child_cursor);
                for mut child in children {
                    traverse_tree(&mut child, source, state);
                }
            }
            "escaped_string_interpolation" => {
                let interpolation_content = node.named_children(&mut cursor).next();
                match interpolation_content {
                    Some(interpolation_content) => {
                        let text = interpolation_content.utf8_text(source).unwrap();
                        push_range(state, "<script>return ", None);
                        push_range(state, text, Some(interpolation_content.range()));
                        push_range(state, ";</script>", None);
                    }
                    None => {}
                }
            }
            "tag_interpolation" => {
                visit_tag_interpolation(&mut cursor, node, source, state);
            }
            "pipe" => {
                visit_pipe(&mut cursor, node, source, state);
            }
            "conditional" => {
                visit_conditional(&mut cursor, node, source, state);
            }
            "tag" => visit_tag(&mut cursor, node, source, state),
            "attributes" => visit_attributes(&mut cursor, node, source, state),
            "content" => {
                for mut interpolation in node.named_children(&mut cursor) {
                    traverse_tree(&mut interpolation, source, state);
                }
                // Always traverse the whole content after we've traversed the interpolation, so they
                // appear after in the conversion ranges
                push_range(state, node.utf8_text(source).unwrap(), Some(node.range()));
            }
            "keyword" | "mixin_attributes" | "comment" => {}
            _ => {
                println!("Unhandled node type: {}", node_type);
            }
        }
    }
}
