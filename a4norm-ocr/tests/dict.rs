// The character list read from the recognizer's inference.yml.
use a4norm_ocr::character_dict;

#[test]
fn plain_and_quoted() {
    let yml = "PostProcess:\n  name: CTCLabelDecode\n  character_dict:\n  - '0'\n  - A\n  - ''''\n  - ' '\n  - ä\n";
    assert_eq!(character_dict(yml), ["0", "A", "'", " ", "ä"]);
}

#[test]
fn the_latin_recognizer() {
    let Ok(yml) = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/models/rec.yml")) else {
        eprintln!("skipped: no models (a4norm-ocr/models.sh)");
        return;
    };
    let d = character_dict(&yml);
    assert_eq!(d.len(), 836);
    for c in ["ä", "ö", "ü", "ß", "@", "§", "€"] {
        assert!(d.iter().any(|x| x == c), "{c}");
    }
}
