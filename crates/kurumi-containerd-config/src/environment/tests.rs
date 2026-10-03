use super::{Token, lex_line};

#[test]
fn tokenizes_assignment_boundaries() {
    assert_eq!(
        lex_line("export KEY='a=b # literal'").collect::<Vec<_>>(),
        [
            Token::Export,
            Token::Key("KEY"),
            Token::Equals,
            Token::Value("a=b # literal"),
        ]
    );
    assert_eq!(
        lex_line("EMPTY=").collect::<Vec<_>>(),
        [Token::Key("EMPTY"), Token::Equals, Token::Value("")]
    );
    assert_eq!(
        lex_line("MISSING").collect::<Vec<_>>(),
        [Token::Key("MISSING")]
    );
    assert!(lex_line("  # comment").next().is_none());
    assert!(lex_line("  ").next().is_none());
}
