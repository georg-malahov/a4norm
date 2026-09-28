//! The languages of a page, by its function words: a form's labels and
//! notes are full of them, and a label in several languages ("Name / name /
//! nom / cognome") carries each language's.

/// Function words per language, and the words a form's labels are made of,
/// lower case. A word on more than one list ("de", "la", "datum",
/// "signature") says nothing and is not counted.
const WORDS: [(&str, &[&str]); 10] = [
    ("de", &["der", "die", "das", "und", "oder", "nicht", "mit", "für", "von", "zu", "den", "dem", "des", "ein",
        "eine", "einer", "ist", "sind", "wird", "werden", "bitte", "ich", "sie", "ihr", "ihre", "auf", "im", "bei",
        "nach", "über", "auch", "als", "wenn", "vom", "zur", "zum", "sowie", "ja", "nein", "vorname",
        "familienname", "geburtsdatum", "geburtsort", "staatsangehörigkeit", "anschrift", "straße", "wohnort",
        "unterschrift", "ort", "geschlecht", "ledig", "verheiratet"]),
    ("en", &["the", "and", "or", "of", "to", "for", "with", "not", "your", "you", "are", "is", "be", "please",
        "if", "this", "that", "on", "by", "from", "have", "has", "yes", "no", "which", "any", "surname",
        "first", "birth", "place", "address", "nationality", "sex", "married", "single"]),
    ("fr", &["le", "la", "les", "et", "ou", "de", "des", "du", "pour", "avec", "vous", "votre", "vos", "est",
        "sont", "une", "un", "au", "aux", "si", "ne", "pas", "oui", "non", "veuillez", "sur", "par", "dans",
        "nom", "prénom", "naissance", "lieu", "adresse", "nationalité", "sexe", "marié", "célibataire"]),
    ("it", &["il", "lo", "la", "gli", "le", "e", "o", "di", "del", "della", "dei", "per", "con", "non", "sono",
        "è", "una", "uno", "si", "suo", "sua", "nel", "nella", "sì", "da", "dal", "alla", "cognome", "nascita",
        "luogo", "indirizzo", "cittadinanza", "sesso", "coniugato", "celibe"]),
    ("es", &["el", "los", "las", "y", "o", "de", "del", "para", "con", "no", "es", "una", "su", "sus", "por",
        "si", "sí", "en", "que", "al", "usted", "apellido", "apellidos", "nacimiento", "lugar", "fecha",
        "dirección", "nacionalidad", "casado", "soltero"]),
    ("pt", &["o", "os", "as", "e", "ou", "de", "do", "da", "dos", "das", "para", "com", "não", "sim", "é", "uma",
        "seu", "sua", "em", "no", "na", "apelido", "nascimento", "morada", "assinatura", "nacionalidade"]),
    ("nl", &["de", "het", "een", "en", "of", "van", "voor", "met", "niet", "uw", "u", "is", "zijn", "ja", "nee",
        "op", "bij", "naar", "naam", "voornaam", "geboortedatum", "handtekening", "adres"]),
    ("pl", &["i", "w", "z", "na", "nie", "jest", "oraz", "lub", "do", "dla", "się", "tak", "od", "po", "pan",
        "pani", "nazwisko", "imię", "urodzenia", "miejsce", "podpis", "obywatelstwo"]),
    ("hr", &["i", "ili", "je", "su", "za", "od", "na", "se", "da", "ne", "u", "iz", "ime", "prezime", "datum",
        "rođenja", "mjesto", "adresa", "potpis", "državljanstvo", "spol"]),
    ("tr", &["ve", "veya", "ile", "için", "bir", "bu", "da", "de", "ne", "değil", "adı", "soyadı", "tarihi",
        "evet", "hayır", "olarak", "ise", "doğum", "yeri", "adresi", "uyruğu", "imza", "cinsiyeti"]),
];

/// The page's languages, most frequent first: each whose own words come to
/// at least 3 and to a quarter of the top language's.
pub fn langs<'a>(words: impl Iterator<Item = &'a str>) -> Vec<&'static str> {
    let own = |t: &str| {
        let mut on = WORDS.iter().enumerate().filter(|(_, (_, list))| list.contains(&t));
        match (on.next(), on.next()) {
            (Some((i, _)), None) => Some(i),
            _ => None,
        }
    };
    let mut hits = [0usize; WORDS.len()];
    for w in words {
        // "Name/name/nom", "(ggf." and "Nein:" hold words too
        for t in w.split(|c: char| !c.is_alphabetic()).filter(|t| !t.is_empty()) {
            if let Some(i) = own(&t.to_lowercase()) {
                hits[i] += 1;
            }
        }
    }
    let top = hits.iter().copied().max().unwrap_or(0);
    let mut out: Vec<(usize, &str)> = WORDS
        .iter()
        .zip(hits)
        .filter(|&(_, n)| n >= 3 && n * 4 >= top)
        .map(|((l, _), n)| (n, *l))
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|(_, l)| l).collect()
}

#[cfg(test)]
mod tests {
    use super::langs;

    #[test]
    fn german_page() {
        let t = "Ich erkläre mich damit einverstanden, dass mein Befund sowie die Abrechnung per E-Mail an die \
                 oben angegebene Adresse gesendet werden. Die Datei wird Ihnen dann zugesandt.";
        assert_eq!(langs(t.split(' ')), ["de"]);
    }

    #[test]
    fn a_label_in_four_languages() {
        let t = "Familienname / surname / nom / cognome Geburtsort / place of birth / lieu de naissance / luogo di \
                 nascita Staatsangehörigkeit / nationality / nationalité / cittadinanza Bitte die Angaben für \
                 den Ehegatten / please enter the details of your spouse / veuillez indiquer les données de \
                 votre conjoint / si prega di indicare i dati del coniuge";
        let l = langs(t.split(' '));
        for want in ["de", "en", "fr", "it"] {
            assert!(l.contains(&want), "{want} in {l:?}");
        }
    }
}
