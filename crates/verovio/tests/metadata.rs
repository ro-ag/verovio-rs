//! Tests for `Toolkit::metadata` — score-level field extraction across
//! MEI, MusicXML, and plaintext formats.

use verovio::Toolkit;

const MEI_WITH_METADATA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mei xmlns="http://www.music-encoding.org/ns/mei" meiversion="4.0.0">
  <meiHead>
    <fileDesc>
      <titleStmt>
        <title>The Test Piece</title>
        <respStmt>
          <persName role="composer">A. Composer</persName>
          <persName role="lyricist">L. Wordsmith</persName>
          <persName role="arranger">R. Arranger</persName>
        </respStmt>
      </titleStmt>
      <pubStmt>
        <availability><useRestrict>Public Domain</useRestrict></availability>
      </pubStmt>
    </fileDesc>
  </meiHead>
  <music><body><mdiv><score>
    <scoreDef><staffGrp>
      <staffDef n="1" lines="5" clef.shape="G" clef.line="2"><label>Violin I</label></staffDef>
      <staffDef n="2" lines="5" clef.shape="F" clef.line="4"><label>Cello</label></staffDef>
    </staffGrp></scoreDef>
    <section><measure>
      <staff n="1"><layer><note pname="g" oct="4" dur="4" xml:id="n1"/></layer></staff>
      <staff n="2"><layer><note pname="c" oct="3" dur="4" xml:id="n2"/></layer></staff>
    </measure></section>
  </score></mdiv></body></music>
</mei>"#;

#[test]
fn mei_metadata_extracted_in_full() {
    let tk = Toolkit::from_data(MEI_WITH_METADATA).expect("load");
    let md = tk.metadata().expect("metadata");
    assert_eq!(md.title.as_deref(), Some("The Test Piece"));
    assert_eq!(md.composer.as_deref(), Some("A. Composer"));
    assert_eq!(md.lyricist.as_deref(), Some("L. Wordsmith"));
    assert_eq!(md.arranger.as_deref(), Some("R. Arranger"));
    assert_eq!(md.copyright.as_deref(), Some("Public Domain"));
    assert_eq!(md.instruments, vec!["Violin I", "Cello"]);
}

const MUSICXML_PARTWISE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <work><work-title>MXL Sample</work-title></work>
  <identification>
    <creator type="composer">Mozart</creator>
    <creator type="lyricist">Da Ponte</creator>
    <rights>Public Domain</rights>
  </identification>
  <part-list>
    <score-part id="P1"><part-name>Flute</part-name></score-part>
    <score-part id="P2"><part-name>Oboe</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>C</step><octave>4</octave></pitch><duration>4</duration><type>whole</type></note>
    </measure>
  </part>
  <part id="P2">
    <measure number="1">
      <attributes>
        <divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>D</step><octave>4</octave></pitch><duration>4</duration><type>whole</type></note>
    </measure>
  </part>
</score-partwise>"#;

#[test]
fn musicxml_metadata_extracted_in_full() {
    let tk = Toolkit::from_data(MUSICXML_PARTWISE).expect("load");
    let md = tk.metadata().expect("metadata");
    assert_eq!(md.title.as_deref(), Some("MXL Sample"));
    assert_eq!(md.composer.as_deref(), Some("Mozart"));
    assert_eq!(md.lyricist.as_deref(), Some("Da Ponte"));
    assert_eq!(md.copyright.as_deref(), Some("Public Domain"));
    assert_eq!(md.instruments, vec!["Flute", "Oboe"]);
}

#[test]
fn pae_metadata_is_mostly_empty() {
    const PAE: &str =
        "@start:s\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:4/4\n@data:'4G/4A/4B/4c\n@end:s\n";
    let tk = Toolkit::from_data(PAE).expect("load");
    let md = tk.metadata().expect("metadata");
    // PAE doesn't carry titles or composer; result is default-ish.
    assert!(md.title.is_none());
    assert!(md.composer.is_none());
}

#[test]
fn metadata_without_load_returns_error() {
    let tk = Toolkit::new();
    let res = tk.metadata();
    assert!(matches!(res, Err(verovio::Error::LoadFailed)));
}
