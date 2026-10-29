//! Role: Decode legacy JSONC without exposing compatibility to normal startup.
pub fn decode(bytes: &[u8]) -> std::io::Result<serde_json::Value> {
    let source = std::str::from_utf8(bytes)
        .map_err(std::io::Error::other)?
        .trim_start_matches('\u{feff}');
    let mut output = source.as_bytes().to_vec();
    let mut index = 0;
    let mut string = false;
    while index < output.len() {
        match output[index] {
            b'\\' if string => {
                index += 2;
                continue;
            }
            b'"' => string = !string,
            b'/' if !string && output.get(index + 1) == Some(&b'/') => {
                output[index] = b' ';
                index += 1;
                while index < output.len() && !matches!(output[index], b'\r' | b'\n') {
                    output[index] = b' ';
                    index += 1;
                }
                continue;
            }
            b'/' if !string && output.get(index + 1) == Some(&b'*') => {
                output[index] = b' ';
                output[index + 1] = b' ';
                index += 2;
                while index + 1 < output.len()
                    && !(output[index] == b'*' && output[index + 1] == b'/')
                {
                    if !matches!(output[index], b'\r' | b'\n') {
                        output[index] = b' ';
                    }
                    index += 1;
                }
                if index + 1 >= output.len() {
                    return Err(std::io::Error::other("unterminated legacy JSONC comment"));
                }
                output[index] = b' ';
                output[index + 1] = b' ';
                index += 2;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    index = 0;
    string = false;
    while index < output.len() {
        match output[index] {
            b'\\' if string => {
                index += 2;
                continue;
            }
            b'"' => string = !string,
            b',' if !string => {
                let next = output[index + 1..]
                    .iter()
                    .find(|byte| !byte.is_ascii_whitespace());
                if matches!(next, Some(b']' | b'}')) {
                    output[index] = b' ';
                }
            }
            _ => {}
        }
        index += 1;
    }
    serde_json::from_slice(&output).map_err(std::io::Error::other)
}
