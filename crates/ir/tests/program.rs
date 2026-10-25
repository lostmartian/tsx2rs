//! Program validation and the binary format.

// Tests may unwrap (docs/engineering/code-standards.md); this covers helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use tsx2rs_ir::{ChunkId, FormatError, MAGIC, Op, OpIndex, PROGRAM_FORMAT_VERSION, Program};

fn chunks(n: usize) -> Vec<Box<[u8]>> {
    (0..n)
        .map(|i| format!("chunk{i}").into_bytes().into())
        .collect()
}

fn stat(html: u32, flight: u32) -> Op {
    Op::Static {
        html: ChunkId(html),
        flight: ChunkId(flight),
        text_start: false,
        text_end: true,
    }
}

#[test]
fn accepts_a_program_with_every_op() {
    let ops = vec![
        stat(0, 1),
        Op::Text,
        Op::Number,
        Op::Attribute { name: ChunkId(2) },
        Op::BooleanAttribute { name: ChunkId(2) },
        Op::RawHtml,
        Op::Value,
        Op::Island,
        Op::Loop {
            end: OpIndex(10),
            separator: Some(ChunkId(1)),
        },
        Op::Text,
        Op::EndLoop,
        Op::If { else_: OpIndex(14) },
        Op::Text,
        Op::Jump { to: OpIndex(15) },
        Op::Number,
    ];
    let program = Program::new(chunks(3), ops.clone()).unwrap();
    assert_eq!(program.ops(), ops.as_slice());
    assert_eq!(program.chunk(ChunkId(1)), b"chunk1");
}

#[test]
#[expect(clippy::too_many_lines, reason = "a table of cases, one per rule")]
fn rejects_programs_that_break_a_rule() {
    let cases: Vec<(&str, Vec<Op>, FormatError)> = vec![
        (
            "a missing chunk",
            vec![stat(0, 5)],
            FormatError::ChunkOutOfRange { op: 0, chunk: 5 },
        ),
        (
            "a missing attribute name",
            vec![Op::Attribute { name: ChunkId(9) }],
            FormatError::ChunkOutOfRange { op: 0, chunk: 9 },
        ),
        (
            "a missing loop separator",
            vec![
                Op::Loop {
                    end: OpIndex(2),
                    separator: Some(ChunkId(9)),
                },
                Op::Text,
                Op::EndLoop,
            ],
            FormatError::ChunkOutOfRange { op: 0, chunk: 9 },
        ),
        (
            "a backward branch",
            vec![Op::Text, Op::If { else_: OpIndex(0) }],
            FormatError::BadTarget { op: 1, target: 0 },
        ),
        (
            "a branch past the end",
            vec![Op::Jump { to: OpIndex(5) }],
            FormatError::BadTarget { op: 0, target: 5 },
        ),
        (
            "a loop end that points backward",
            vec![
                Op::Text,
                Op::Loop {
                    end: OpIndex(0),
                    separator: None,
                },
            ],
            FormatError::BadTarget { op: 1, target: 0 },
        ),
        (
            "a loop end that isn't its EndLoop",
            vec![
                Op::Loop {
                    end: OpIndex(1),
                    separator: None,
                },
                Op::Text,
                Op::EndLoop,
            ],
            FormatError::UnmatchedEndLoop { op: 2 },
        ),
        (
            "an EndLoop without a Loop",
            vec![Op::Text, Op::EndLoop],
            FormatError::UnmatchedEndLoop { op: 1 },
        ),
        (
            "a Loop without an EndLoop",
            vec![
                Op::Loop {
                    end: OpIndex(1),
                    separator: None,
                },
                Op::Text,
            ],
            FormatError::UnclosedLoop { op: 0 },
        ),
        (
            "a loop that reads nothing",
            vec![
                Op::Loop {
                    end: OpIndex(2),
                    separator: None,
                },
                stat(0, 0),
                Op::EndLoop,
            ],
            FormatError::LoopReadsNothing { op: 0 },
        ),
        (
            "a branch out of a loop",
            vec![
                Op::Loop {
                    end: OpIndex(3),
                    separator: None,
                },
                Op::If { else_: OpIndex(4) },
                Op::Text,
                Op::EndLoop,
                Op::Text,
            ],
            FormatError::BadTarget { op: 1, target: 4 },
        ),
        (
            "a branch into a later loop",
            vec![
                Op::If { else_: OpIndex(2) },
                Op::Loop {
                    end: OpIndex(3),
                    separator: None,
                },
                Op::Text,
                Op::EndLoop,
            ],
            FormatError::BadTarget { op: 0, target: 2 },
        ),
        (
            "a branch onto a later loop's end",
            vec![
                Op::Jump { to: OpIndex(3) },
                Op::Loop {
                    end: OpIndex(3),
                    separator: None,
                },
                Op::Text,
                Op::EndLoop,
            ],
            FormatError::BadTarget { op: 0, target: 3 },
        ),
    ];
    for (name, ops, expected) in cases {
        assert_eq!(Program::new(chunks(1), ops), Err(expected), "{name}");
    }
}

#[test]
fn a_branch_may_end_its_loop_body_or_skip_a_whole_loop() {
    let to_end_of_body = vec![
        Op::Loop {
            end: OpIndex(3),
            separator: None,
        },
        Op::If { else_: OpIndex(3) },
        Op::Text,
        Op::EndLoop,
    ];
    assert!(Program::new(chunks(1), to_end_of_body).is_ok());
    let over_a_loop = vec![
        Op::If { else_: OpIndex(4) },
        Op::Loop {
            end: OpIndex(3),
            separator: None,
        },
        Op::Text,
        Op::EndLoop,
        Op::Text,
    ];
    assert!(Program::new(chunks(1), over_a_loop).is_ok());
}

#[test]
fn encodes_and_decodes_exactly() {
    let program = Program::new(
        chunks(3),
        vec![
            Op::Static {
                html: ChunkId(0),
                flight: ChunkId(1),
                text_start: true,
                text_end: false,
            },
            Op::Loop {
                end: OpIndex(3),
                separator: None,
            },
            Op::Island,
            Op::EndLoop,
            Op::Loop {
                end: OpIndex(6),
                separator: Some(ChunkId(0)),
            },
            Op::Value,
            Op::EndLoop,
        ],
    )
    .unwrap();
    let bytes = program.encode().unwrap();
    assert!(bytes.starts_with(MAGIC));
    assert_eq!(Program::decode(&bytes), Ok(program));
}

#[test]
fn rejects_malformed_files() {
    let good = Program::new(chunks(1), vec![stat(0, 0), Op::Text])
        .unwrap()
        .encode()
        .unwrap();
    let with = |f: &dyn Fn(&mut Vec<u8>)| {
        let mut bytes = good.clone();
        f(&mut bytes);
        Program::decode(&bytes)
    };
    assert_eq!(with(&|b| b[0] = b'X'), Err(FormatError::NotAProgram));
    assert_eq!(
        with(&|b| b[8..10].copy_from_slice(&(PROGRAM_FORMAT_VERSION + 1).to_le_bytes())),
        Err(FormatError::UnsupportedVersion {
            found: PROGRAM_FORMAT_VERSION + 1
        })
    );
    assert!(matches!(
        with(&|b| b.truncate(b.len() - 1)),
        Err(FormatError::Truncated { .. })
    ));
    assert!(matches!(
        with(&|b| b.push(0)),
        Err(FormatError::TrailingBytes { .. })
    ));
    let last = good.len() - 1;
    assert_eq!(
        with(&|b| b[last] = 99),
        Err(FormatError::BadOpcode {
            offset: last,
            opcode: 99
        })
    );
    assert!(matches!(
        Program::decode(b"TSX"),
        Err(FormatError::Truncated { .. })
    ));
}

#[test]
fn rejects_unknown_static_flags() {
    let mut bytes = Program::new(chunks(1), vec![stat(0, 0)])
        .unwrap()
        .encode()
        .unwrap();
    let flags = bytes.len() - 1;
    bytes[flags] = 0b100;
    assert!(matches!(
        Program::decode(&bytes),
        Err(FormatError::BadOpcode { .. })
    ));
}

/// A structured template, turned into ops; always valid by construction.
#[derive(Clone, Debug)]
enum Node {
    Static(u32),
    Read(u8),
    Loop(Vec<Node>, bool),
    If(Vec<Node>, Vec<Node>),
}

fn node() -> impl Strategy<Value = Node> {
    let leaf = prop_oneof![
        (0u32..3).prop_map(Node::Static),
        (0u8..7).prop_map(Node::Read)
    ];
    leaf.prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            (prop::collection::vec(inner.clone(), 0..4), any::<bool>())
                .prop_map(|(body, sep)| Node::Loop(body, sep)),
            (
                prop::collection::vec(inner.clone(), 0..3),
                prop::collection::vec(inner, 0..3)
            )
                .prop_map(|(then, otherwise)| Node::If(then, otherwise)),
        ]
    })
}

fn lower(nodes: &[Node], ops: &mut Vec<Op>) {
    let index = |ops: &Vec<Op>| OpIndex(u32::try_from(ops.len()).unwrap());
    for node in nodes {
        match node {
            Node::Static(c) => ops.push(stat(*c, *c)),
            Node::Read(kind) => ops.push(match kind {
                0 => Op::Text,
                1 => Op::Number,
                2 => Op::Attribute { name: ChunkId(0) },
                3 => Op::BooleanAttribute { name: ChunkId(1) },
                4 => Op::RawHtml,
                5 => Op::Value,
                _ => Op::Island,
            }),
            Node::Loop(body, sep) => {
                let start = ops.len();
                ops.push(Op::EndLoop); // placeholder
                ops.push(Op::Text); // every loop body reads input
                lower(body, ops);
                let end = index(ops);
                ops.push(Op::EndLoop);
                ops[start] = Op::Loop {
                    end,
                    separator: sep.then_some(ChunkId(2)),
                };
            }
            Node::If(then, otherwise) => {
                let branch = ops.len();
                ops.push(Op::EndLoop); // placeholder
                lower(then, ops);
                let jump = ops.len();
                ops.push(Op::EndLoop); // placeholder
                let else_ = index(ops);
                lower(otherwise, ops);
                ops[branch] = Op::If { else_ };
                ops[jump] = Op::Jump { to: index(ops) };
            }
        }
    }
}

proptest! {
    #[test]
    fn structured_programs_are_valid_and_round_trip(nodes in prop::collection::vec(node(), 0..8)) {
        let mut ops = Vec::new();
        lower(&nodes, &mut ops);
        let program = Program::new(chunks(3), ops).unwrap();
        prop_assert_eq!(Program::decode(&program.encode().unwrap()), Ok(program));
    }

    #[test]
    fn decoding_arbitrary_bytes_never_panics(mut bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        // Half the cases get a valid header, so decoding goes past the magic check.
        if bytes.first().is_some_and(|b| b % 2 == 0) {
            let mut header = MAGIC.to_vec();
            header.extend_from_slice(&PROGRAM_FORMAT_VERSION.to_le_bytes());
            header.append(&mut bytes);
            bytes = header;
        }
        let _ = Program::decode(&bytes);
    }
}
