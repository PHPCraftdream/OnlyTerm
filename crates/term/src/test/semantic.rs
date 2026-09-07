use super::*;

#[test]
fn test_semantic_1539() {
    use onlyterm_escape_parser::osc::FinalTermSemanticPrompt;
    let mut term = TestTerm::new(5, 10, 0);
    term.print(format!(
        "{}prompt\r\nwoot",
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfPromptAndStartOfInputUntilEndOfLine
        )
    ));

    assert_visible_contents(&term, file!(), line!(), &["prompt", "woot", "", "", ""]);

    k9::snapshot!(
        term.get_semantic_zones().unwrap(),
        "
[
    SemanticZone {
        start_y: 0,
        start_x: 0,
        end_y: 0,
        end_x: 5,
        semantic_type: Input,
    },
    SemanticZone {
        start_y: 1,
        start_x: 0,
        end_y: 1,
        end_x: 3,
        semantic_type: Output,
    },
]
"
    );
}

#[test]
fn test_semantic() {
    use onlyterm_escape_parser::osc::FinalTermSemanticPrompt;
    let mut term = TestTerm::new(5, 10, 0);
    term.print("hello");
    term.print(format!(
        "{}",
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::FreshLine)
    ));
    term.print("there");

    assert_visible_contents(&term, file!(), line!(), &["hello", "there", "", "", ""]);

    term.cup(0, 2);
    term.print(format!(
        "{}",
        OperatingSystemCommand::FinalTermSemanticPrompt(FinalTermSemanticPrompt::FreshLine)
    ));
    term.print("three");
    assert_visible_contents(
        &term,
        file!(),
        line!(),
        &["hello", "there", "three", "", ""],
    );

    k9::snapshot!(
        term.get_semantic_zones().unwrap(),
        "
[
    SemanticZone {
        start_y: 0,
        start_x: 0,
        end_y: 2,
        end_x: 4,
        semantic_type: Output,
    },
]
"
    );

    term.print(format!(
        "{}",
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::FreshLineAndStartPrompt {
                aid: None,
                cl: None
            }
        )
    ));
    term.print("> ");
    term.print(format!(
        "{}",
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfPromptAndStartOfInputUntilNextMarker
        )
    ));
    term.print("ls -l\r\n");
    term.print(format!(
        "{}",
        OperatingSystemCommand::FinalTermSemanticPrompt(
            FinalTermSemanticPrompt::MarkEndOfInputAndStartOfOutput { aid: None }
        )
    ));
    term.print("some file");

    let output = CellAttributes::default();
    let mut input = CellAttributes::default();
    input.set_semantic_type(SemanticType::Input);

    let mut prompt_line = Line::from_text("> ls -l", &output, SEQ_ZERO, None);
    for i in 0..2 {
        prompt_line.cells_mut()[i]
            .attrs_mut()
            .set_semantic_type(SemanticType::Prompt);
    }
    for i in 2..7 {
        prompt_line.cells_mut()[i]
            .attrs_mut()
            .set_semantic_type(SemanticType::Input);
    }

    k9::snapshot!(
        term.get_semantic_zones().unwrap(),
        "
[
    SemanticZone {
        start_y: 0,
        start_x: 0,
        end_y: 2,
        end_x: 4,
        semantic_type: Output,
    },
    SemanticZone {
        start_y: 3,
        start_x: 0,
        end_y: 3,
        end_x: 1,
        semantic_type: Prompt,
    },
    SemanticZone {
        start_y: 3,
        start_x: 2,
        end_y: 3,
        end_x: 6,
        semantic_type: Input,
    },
    SemanticZone {
        start_y: 4,
        start_x: 0,
        end_y: 4,
        end_x: 8,
        semantic_type: Output,
    },
]
"
    );

    assert_lines_equal(
        file!(),
        line!(),
        &term.screen().visible_lines(),
        &[
            Line::from_text("hello", &output, SEQ_ZERO, None),
            Line::from_text("there", &output, SEQ_ZERO, None),
            Line::from_text("three", &output, SEQ_ZERO, None),
            prompt_line,
            Line::from_text("some file", &output, SEQ_ZERO, None),
        ],
        Compare::TEXT | Compare::ATTRS,
    );
}
