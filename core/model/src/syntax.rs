//! Role: Describe built-in syntax recognition, rendering names, and completions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinSyntaxKind {
    Admonition,
    Alert,
    Mermaid,
    Abc,
    Math,
    Youtube,
    Tabs,
    Tab,
    Details,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxRecognition<'a> {
    Fence(&'a str),
    Directive(&'a str),
    Alert(&'a str),
    Youtube,
}
pub struct BuiltinSyntaxDescriptor {
    pub name: &'static str,
    pub kind: BuiltinSyntaxKind,
    pub recognizers: &'static [SyntaxRecognition<'static>],
    pub render_kind: &'static str,
    pub default_title: &'static str,
    pub completions: &'static [SyntaxCompletion],
}
#[derive(Clone, Copy)]
pub struct SyntaxCompletion {
    pub label: &'static str,
    pub filter_text: &'static str,
    pub detail: &'static str,
    pub snippet: &'static str,
    pub plain_text: &'static str,
}

pub const BUILTIN_SYNTAX: &[BuiltinSyntaxDescriptor] = &[
    BuiltinSyntaxDescriptor {
        name: "admonition",
        kind: BuiltinSyntaxKind::Admonition,
        recognizers: &[SyntaxRecognition::Directive("admonition")],
        render_kind: BuiltinSyntaxKind::Admonition.render_kind(),
        default_title: "Admonition",
        completions: &[
            SyntaxCompletion {
                label: "admonition",
                filter_text: ":::admonition",
                detail: "Admonition block",
                snippet: ":::${1|info,tip,important,warning,danger|}\n\n${2:Content}\n\n:::",
                plain_text: ":::info\n\nContent\n\n:::",
            },
            SyntaxCompletion {
                label: "admonition with title",
                filter_text: ":::admonition",
                detail: "Admonition block with title",
                snippet: ":::${1|info,tip,important,warning,danger|}[${2:Title}]\n\n${3:Content}\n\n:::",
                plain_text: ":::info[Title]\n\nContent\n\n:::",
            },
        ],
    },
    BuiltinSyntaxDescriptor {
        name: "info",
        kind: BuiltinSyntaxKind::Admonition,
        recognizers: &[SyntaxRecognition::Directive("info")],
        render_kind: BuiltinSyntaxKind::Admonition.render_kind(),
        default_title: "Info",
        completions: &[
            SyntaxCompletion {
                label: "info",
                filter_text: ":::info",
                detail: "Info block",
                snippet: ":::info\n\n${1:Content}\n\n:::",
                plain_text: ":::info\n\nContent\n\n:::",
            },
            SyntaxCompletion {
                label: "info with title",
                filter_text: ":::info",
                detail: "Info block with title",
                snippet: ":::info[${1:Title}]\n\n${2:Content}\n\n:::",
                plain_text: ":::info[Title]\n\nContent\n\n:::",
            },
        ],
    },
    BuiltinSyntaxDescriptor {
        name: "tip",
        kind: BuiltinSyntaxKind::Admonition,
        recognizers: &[SyntaxRecognition::Directive("tip")],
        render_kind: BuiltinSyntaxKind::Admonition.render_kind(),
        default_title: "Tip",
        completions: &[
            SyntaxCompletion {
                label: "tip",
                filter_text: ":::tip",
                detail: "Tip block",
                snippet: ":::tip\n\n${1:Content}\n\n:::",
                plain_text: ":::tip\n\nContent\n\n:::",
            },
            SyntaxCompletion {
                label: "tip with title",
                filter_text: ":::tip",
                detail: "Tip block with title",
                snippet: ":::tip[${1:Title}]\n\n${2:Content}\n\n:::",
                plain_text: ":::tip[Title]\n\nContent\n\n:::",
            },
        ],
    },
    BuiltinSyntaxDescriptor {
        name: "important",
        kind: BuiltinSyntaxKind::Admonition,
        recognizers: &[SyntaxRecognition::Directive("important")],
        render_kind: BuiltinSyntaxKind::Admonition.render_kind(),
        default_title: "Important",
        completions: &[
            SyntaxCompletion {
                label: "important",
                filter_text: ":::important",
                detail: "Important block",
                snippet: ":::important\n\n${1:Content}\n\n:::",
                plain_text: ":::important\n\nContent\n\n:::",
            },
            SyntaxCompletion {
                label: "important with title",
                filter_text: ":::important",
                detail: "Important block with title",
                snippet: ":::important[${1:Title}]\n\n${2:Content}\n\n:::",
                plain_text: ":::important[Title]\n\nContent\n\n:::",
            },
        ],
    },
    BuiltinSyntaxDescriptor {
        name: "warning",
        kind: BuiltinSyntaxKind::Admonition,
        recognizers: &[SyntaxRecognition::Directive("warning")],
        render_kind: BuiltinSyntaxKind::Admonition.render_kind(),
        default_title: "Warning",
        completions: &[
            SyntaxCompletion {
                label: "warning",
                filter_text: ":::warning",
                detail: "Warning block",
                snippet: ":::warning\n\n${1:Content}\n\n:::",
                plain_text: ":::warning\n\nContent\n\n:::",
            },
            SyntaxCompletion {
                label: "warning with title",
                filter_text: ":::warning",
                detail: "Warning block with title",
                snippet: ":::warning[${1:Title}]\n\n${2:Content}\n\n:::",
                plain_text: ":::warning[Title]\n\nContent\n\n:::",
            },
        ],
    },
    BuiltinSyntaxDescriptor {
        name: "danger",
        kind: BuiltinSyntaxKind::Admonition,
        recognizers: &[SyntaxRecognition::Directive("danger")],
        render_kind: BuiltinSyntaxKind::Admonition.render_kind(),
        default_title: "Danger",
        completions: &[
            SyntaxCompletion {
                label: "danger",
                filter_text: ":::danger",
                detail: "Danger block",
                snippet: ":::danger\n\n${1:Content}\n\n:::",
                plain_text: ":::danger\n\nContent\n\n:::",
            },
            SyntaxCompletion {
                label: "danger with title",
                filter_text: ":::danger",
                detail: "Danger block with title",
                snippet: ":::danger[${1:Title}]\n\n${2:Content}\n\n:::",
                plain_text: ":::danger[Title]\n\nContent\n\n:::",
            },
        ],
    },
    BuiltinSyntaxDescriptor {
        name: "tabs",
        kind: BuiltinSyntaxKind::Tabs,
        recognizers: &[SyntaxRecognition::Directive("tabs")],
        render_kind: BuiltinSyntaxKind::Tabs.render_kind(),
        default_title: "Tabs",
        completions: &[SyntaxCompletion {
            label: "tabs",
            filter_text: ":::tabs",
            detail: "Tabs block",
            snippet: "::::tabs\n\n  :::tab[${1:Tab 1}]\n\n  ${2:Content}\n\n  :::\n\n::::",
            plain_text: "::::tabs\n\n  :::tab[Tab 1]\n\n  Content\n\n  :::\n\n::::",
        }],
    },
    BuiltinSyntaxDescriptor {
        name: "details",
        kind: BuiltinSyntaxKind::Details,
        recognizers: &[SyntaxRecognition::Directive("details")],
        render_kind: BuiltinSyntaxKind::Details.render_kind(),
        default_title: "Details",
        completions: &[
            SyntaxCompletion {
                label: "details",
                filter_text: ":::details",
                detail: "Details block",
                snippet: ":::details\n\n${1:Content}\n\n:::",
                plain_text: ":::details\n\nContent\n\n:::",
            },
            SyntaxCompletion {
                label: "details with title",
                filter_text: ":::details",
                detail: "Details block with title",
                snippet: ":::details[${1:Title}]\n\n${2:Content}\n\n:::",
                plain_text: ":::details[Title]\n\nContent\n\n:::",
            },
        ],
    },
    BuiltinSyntaxDescriptor {
        name: "note",
        kind: BuiltinSyntaxKind::Alert,
        recognizers: &[SyntaxRecognition::Alert("NOTE")],
        render_kind: BuiltinSyntaxKind::Alert.render_kind(),
        default_title: "Note",
        completions: &[SyntaxCompletion {
            label: "alert note",
            filter_text: "> [!NOTE]",
            detail: "GitHub NOTE alert",
            snippet: "> [!NOTE]\n> ${1:Content}",
            plain_text: "> [!NOTE]\n> Content",
        }],
    },
    BuiltinSyntaxDescriptor {
        name: "tip",
        kind: BuiltinSyntaxKind::Alert,
        recognizers: &[SyntaxRecognition::Alert("TIP")],
        render_kind: BuiltinSyntaxKind::Alert.render_kind(),
        default_title: "Tip",
        completions: &[SyntaxCompletion {
            label: "alert tip",
            filter_text: "> [!TIP]",
            detail: "GitHub TIP alert",
            snippet: "> [!TIP]\n> ${1:Content}",
            plain_text: "> [!TIP]\n> Content",
        }],
    },
    BuiltinSyntaxDescriptor {
        name: "important",
        kind: BuiltinSyntaxKind::Alert,
        recognizers: &[SyntaxRecognition::Alert("IMPORTANT")],
        render_kind: BuiltinSyntaxKind::Alert.render_kind(),
        default_title: "Important",
        completions: &[SyntaxCompletion {
            label: "alert important",
            filter_text: "> [!IMPORTANT]",
            detail: "GitHub IMPORTANT alert",
            snippet: "> [!IMPORTANT]\n> ${1:Content}",
            plain_text: "> [!IMPORTANT]\n> Content",
        }],
    },
    BuiltinSyntaxDescriptor {
        name: "warning",
        kind: BuiltinSyntaxKind::Alert,
        recognizers: &[SyntaxRecognition::Alert("WARNING")],
        render_kind: BuiltinSyntaxKind::Alert.render_kind(),
        default_title: "Warning",
        completions: &[SyntaxCompletion {
            label: "alert warning",
            filter_text: "> [!WARNING]",
            detail: "GitHub WARNING alert",
            snippet: "> [!WARNING]\n> ${1:Content}",
            plain_text: "> [!WARNING]\n> Content",
        }],
    },
    BuiltinSyntaxDescriptor {
        name: "caution",
        kind: BuiltinSyntaxKind::Alert,
        recognizers: &[SyntaxRecognition::Alert("CAUTION")],
        render_kind: BuiltinSyntaxKind::Alert.render_kind(),
        default_title: "Caution",
        completions: &[SyntaxCompletion {
            label: "alert caution",
            filter_text: "> [!CAUTION]",
            detail: "GitHub CAUTION alert",
            snippet: "> [!CAUTION]\n> ${1:Content}",
            plain_text: "> [!CAUTION]\n> Content",
        }],
    },
    BuiltinSyntaxDescriptor {
        name: "abc",
        kind: BuiltinSyntaxKind::Abc,
        recognizers: &[SyntaxRecognition::Fence("abc")],
        render_kind: BuiltinSyntaxKind::Abc.render_kind(),
        default_title: "Abc",
        completions: &[
            SyntaxCompletion {
                label: "abc",
                filter_text: "```abc",
                detail: "ABC notation template",
                snippet: "```abc\nX:1\nT:${1:Title}\nM:${2|4/4,3/4,6/8,12/8,2/2|}\nL:${3|1/8,1/4,1/16|}\nK:${4:C}\nQ:${5:120}\nV:1\n\n${6}\n```",
                plain_text: "```abc\nX:1\nT:Title\nM:4/4\nL:1/8\nK:C\nQ:120\nV:1\n\n\n```",
            },
            SyntaxCompletion {
                label: "abc piano",
                filter_text: "```abc piano",
                detail: "Piano ABC notation template",
                snippet: "```abc\nX:1\nT:${1:Title}\nM:${2|4/4,3/4,6/8,12/8,2/2|}\nL:${3|1/4,1/8,1/16|}\nK:${4:C}\nQ:${5:120}\n%%staves {(rh) (lh)}\nV:rh clef=treble name=\"Piano\" snm=\"Pno\"\nV:lh clef=bass octave=-2\n% Start of the melody\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n% next line\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n```",
                plain_text: "```abc\nX:1\nT:Title\nM:4/4\nL:1/4\nK:C\nQ:120\n%%staves {(rh) (lh)}\nV:rh clef=treble name=\"Piano\" snm=\"Pno\"\nV:lh clef=bass octave=-2\n% Start of the melody\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n% next line\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n```",
            },
            SyntaxCompletion {
                label: "abc piano-vocal",
                filter_text: "```abc piano-vocal",
                detail: "Piano and vocal ABC notation template",
                snippet: "```abc\nX:1\nT:${1:Title}\nM:${2|4/4,3/4,6/8,12/8,2/2|}\nL:${3|1/4,1/8,1/16|}\nK:${4:C}\nQ:${5:120}\n%%staves (ml) {(rh) (lh)}\nV:ml clef=treble name=\"Melody\" snm=\"M\"\n%%MIDI program 80\nV:rh clef=treble name=\"Chord\" snm=\"C\"\n%%MIDI program 0\nV:lh clef=bass octave=-2\n% Start of the melody\n[V:ml]\nC D E F | G z G z | | |\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n% next line\n[V:ml]\nC D E F | G z G z | | |\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n```",
                plain_text: "```abc\nX:1\nT:Title\nM:4/4\nL:1/4\nK:C\nQ:120\n%%staves (ml) {(rh) (lh)}\nV:ml clef=treble name=\"Melody\" snm=\"M\"\n%%MIDI program 80\nV:rh clef=treble name=\"Chord\" snm=\"C\"\n%%MIDI program 0\nV:lh clef=bass octave=-2\n% Start of the melody\n[V:ml]\nC D E F | G z G z | | |\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n% next line\n[V:ml]\nC D E F | G z G z | | |\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n```",
            },
            SyntaxCompletion {
                label: "abc satb",
                filter_text: "```abc satb",
                detail: "SATB ABC notation template",
                snippet: "```abc\nX:1\nT:${1:Title}\nM:${2|4/4,3/4,6/8,12/8,2/2|}\nL:${3|1/4,1/8,1/16|}\nK:${4:C}\nQ:${5:120}\n%%staves { (S A) (T B) }\nV:S name=\"Soprano\" snm=\"S\" clef=treble\nV:A name=\"Alto\" snm=\"A\" clef=treble\nV:T name=\"Tenor\" snm=\"T\" clef=bass octave=-1\nV:B name=\"Bass\" snm=\"B\" clef=bass octave=-1\n% Start of the melody\n[V:S]\nE F G A | B c d e | | |\n[V:A]\nC D E F | G A B c | | |\n[V:T]\nG A B c | d e f g | | |\n[V:B]\nC D E F | G A B c | | |\n% next line\n[V:S]\nE F G A | B c d e | | |\n[V:A]\nC D E F | G A B c | | |\n[V:T]\nG A B c | d e f g | | |\n[V:B]\nC D E F | G A B c | | |\n```",
                plain_text: "```abc\nX:1\nT:Title\nM:4/4\nL:1/4\nK:C\nQ:120\n%%staves { (S A) (T B) }\nV:S name=\"Soprano\" snm=\"S\" clef=treble\nV:A name=\"Alto\" snm=\"A\" clef=treble\nV:T name=\"Tenor\" snm=\"T\" clef=bass octave=-1\nV:B name=\"Bass\" snm=\"B\" clef=bass octave=-1\n% Start of the melody\n[V:S]\nE F G A | B c d e | | |\n[V:A]\nC D E F | G A B c | | |\n[V:T]\nG A B c | d e f g | | |\n[V:B]\nC D E F | G A B c | | |\n% next line\n[V:S]\nE F G A | B c d e | | |\n[V:A]\nC D E F | G A B c | | |\n[V:T]\nG A B c | d e f g | | |\n[V:B]\nC D E F | G A B c | | |\n```",
            },
        ],
    },
    BuiltinSyntaxDescriptor {
        name: "mermaid",
        kind: BuiltinSyntaxKind::Mermaid,
        recognizers: &[SyntaxRecognition::Fence("mermaid")],
        render_kind: BuiltinSyntaxKind::Mermaid.render_kind(),
        default_title: "Mermaid",
        completions: &[SyntaxCompletion {
            label: "mermaid",
            filter_text: "```mermaid",
            detail: "Mermaid diagram",
            snippet: "```mermaid\n${1:graph TD}\n```",
            plain_text: "```mermaid\ngraph TD\n```",
        }],
    },
    BuiltinSyntaxDescriptor {
        name: "math",
        kind: BuiltinSyntaxKind::Math,
        recognizers: &[SyntaxRecognition::Fence("math")],
        render_kind: BuiltinSyntaxKind::Math.render_kind(),
        default_title: "Math",
        completions: &[SyntaxCompletion {
            label: "math",
            filter_text: "```math",
            detail: "Display math block",
            snippet: "```math\n${1:formula}\n```",
            plain_text: "```math\nformula\n```",
        }],
    },
    BuiltinSyntaxDescriptor {
        name: "tab",
        kind: BuiltinSyntaxKind::Tab,
        recognizers: &[SyntaxRecognition::Directive("tab")],
        render_kind: BuiltinSyntaxKind::Tab.render_kind(),
        default_title: "Tab",
        completions: &[SyntaxCompletion {
            label: "tab",
            filter_text: ":::tab",
            detail: "Tab block",
            snippet: ":::tab[${1:Title}]\n\n${2:Content}\n\n:::",
            plain_text: ":::tab[Title]\n\nContent\n\n:::",
        }],
    },
    BuiltinSyntaxDescriptor {
        name: "youtube",
        kind: BuiltinSyntaxKind::Youtube,
        recognizers: &[SyntaxRecognition::Youtube],
        render_kind: BuiltinSyntaxKind::Youtube.render_kind(),
        default_title: "YouTube",
        completions: &[SyntaxCompletion {
            label: "youtube",
            filter_text: "https://youtu.be/",
            detail: "YouTube video",
            snippet: "https://youtu.be/${1:dQw4w9WgXcQ}",
            plain_text: "https://youtu.be/dQw4w9WgXcQ",
        }],
    },
];

pub fn lookup_fence(name: &str) -> Option<&'static BuiltinSyntaxDescriptor> {
    lookup(SyntaxRecognition::Fence(name))
}
pub fn lookup_directive(name: &str) -> Option<&'static BuiltinSyntaxDescriptor> {
    lookup(SyntaxRecognition::Directive(name))
}
pub fn lookup_alert(name: &str) -> Option<&'static BuiltinSyntaxDescriptor> {
    BUILTIN_SYNTAX.iter().find(|descriptor| descriptor.recognizers.iter().any(|recognizer| matches!(recognizer, SyntaxRecognition::Alert(value) if value.eq_ignore_ascii_case(name))))
}
fn lookup(recognizer: SyntaxRecognition<'_>) -> Option<&'static BuiltinSyntaxDescriptor> {
    BUILTIN_SYNTAX
        .iter()
        .find(|descriptor| descriptor.recognizers.contains(&recognizer))
}
/// Extract a complete YouTube video identifier from an HTTPS watch or short URL.
pub fn youtube_video_id(source: &str) -> Option<&str> {
    let (scheme, rest) = source.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let (host, path) = rest.split_once('/')?;
    let candidate = if host.eq_ignore_ascii_case("youtu.be") {
        path.split(['?', '#']).next()?
    } else if host.eq_ignore_ascii_case("youtube.com")
        || host.eq_ignore_ascii_case("www.youtube.com")
    {
        let query = path.strip_prefix("watch?")?.split('#').next()?;
        query.split('&').find_map(|part| part.strip_prefix("v="))?
    } else {
        return None;
    };
    (candidate.len() == 11
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then_some(candidate)
}

impl BuiltinSyntaxKind {
    pub const fn render_kind(self) -> &'static str {
        match self {
            Self::Admonition | Self::Alert => "admonition",
            Self::Mermaid => "mermaid",
            Self::Abc => "abc",
            Self::Math => "math",
            Self::Youtube => "youtube",
            Self::Tabs => "tabs",
            Self::Tab => "tab",
            Self::Details => "details",
        }
    }
}

#[cfg(test)]
mod tests {
    //! Role: Verify syntax metadata and strict YouTube video identifiers.
    use super::*;
    #[test]
    fn descriptor_render_names_match_their_kinds() {
        for descriptor in BUILTIN_SYNTAX {
            assert!(!descriptor.completions.is_empty());
            assert_eq!(descriptor.render_kind, descriptor.kind.render_kind());
        }
    }
    #[test]
    fn youtube_identifiers_require_a_complete_trusted_host_and_safe_id() {
        for source in [
            "https://youtu.be/dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&feature=share",
            "https://youtube.com/watch?x=1&v=dQw4w9WgXcQ",
            "HTTPS://YOUTU.BE/dQw4w9WgXcQ?t=1",
        ] {
            assert_eq!(youtube_video_id(source), Some("dQw4w9WgXcQ"));
        }
        for source in [
            "http://youtu.be/dQw4w9WgXcQ",
            "https://youtu.be/short",
            "https://youtu.be/dQw4w9WgXcQ/extra",
            "https://youtube.com.evil/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?v=dQw4w9WgXc!",
            "https://youtube.com/watch?x=dQw4w9WgXcQ",
        ] {
            assert_eq!(youtube_video_id(source), None);
        }
    }
}
