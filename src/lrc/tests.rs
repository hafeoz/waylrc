use super::*;

#[test]
fn example() {
    const LYRIC: &[u8] = r#"[00:12.00]Line 1 lyrics
[00:17.20]Line 2 lyrics"#
        .as_bytes();

    let lrc = Lrc::from_reader(LYRIC).unwrap();

    assert_eq!(
        lrc,
        Lrc(vec![vec![
            (
                TimeTag(Duration::from_secs(12)),
                "Line 1 lyrics".to_string(),
            ),
            (
                TimeTag(Duration::from_secs(17) + Duration::from_millis(200)),
                "Line 2 lyrics".to_string(),
            )
        ]
        .into_iter()
        .collect()])
    );
}

#[test]
fn repeating_lyrics() {
    const LYRIC: &[u8] = r#"[00:12.00]Line 1 lyrics
[00:21.10][00:45.10]Repeating lyrics (e.g. chorus)"#
        .as_bytes();

    let lrc = Lrc::from_reader(LYRIC).unwrap();

    assert_eq!(
        lrc,
        Lrc(vec![vec![
            (
                TimeTag(Duration::from_secs(12)),
                "Line 1 lyrics".to_string(),
            ),
            (
                TimeTag(Duration::from_secs(21) + Duration::from_millis(100)),
                "Repeating lyrics (e.g. chorus)".to_string(),
            ),
            (
                TimeTag(Duration::from_secs(45) + Duration::from_millis(100)),
                "Repeating lyrics (e.g. chorus)".to_string(),
            )
        ]
        .into_iter()
        .collect()])
    );
}

#[test]
fn walaoke_extension() {
    const LYRIC: &[u8] = r#"[00:12.00]Line 1 lyrics
[00:17.20]F: Line 2 lyrics
[00:21.10]M: Line 3 lyrics
[00:24.00]Line 4 lyrics
[00:28.25]D: Line 5 lyrics
[00:29.02]Line 6 lyrics"#
        .as_bytes();

    let lrc = Lrc::from_reader(LYRIC).unwrap();

    assert_eq!(
        lrc,
        Lrc(vec![vec![
            (
                TimeTag(Duration::from_secs(12)),
                "Line 1 lyrics".to_string(),
            ),
            (
                TimeTag(Duration::from_secs(17) + Duration::from_millis(200)),
                "Line 2 lyrics".to_string(),
            ),
            (
                TimeTag(Duration::from_secs(21) + Duration::from_millis(100)),
                "Line 3 lyrics".to_string(),
            ),
            (
                TimeTag(Duration::from_secs(24)),
                "Line 4 lyrics".to_string(),
            ),
            (
                TimeTag(Duration::from_secs(28) + Duration::from_millis(250)),
                "Line 5 lyrics".to_string(),
            ),
            (
                TimeTag(Duration::from_secs(29) + Duration::from_millis(20)),
                "Line 6 lyrics".to_string(),
            )
        ]
        .into_iter()
        .collect()])
    );
}

#[test]
fn exhanced_lrc() {
    const LYRIC: &[u8] = r#"[ar: Jefferson Airplane]
[al: Surrealistic Pillow]
[au: Jefferson Airplane]
[length: 2:58]
[by: lrc-maker]
[ti: Somebody to Love]

[00:00.00] <00:00.04> When <00:00.16> the <00:00.82> truth <00:01.29> is <00:01.63> found <00:03.09> to <00:03.37> be <00:05.92> lies
[00:06.47] <00:07.67> And <00:07.94> all <00:08.36> the <00:08.63> joy <00:10.28> within <00:10.53> you <00:13.09> dies
[00:13.34] <00:14.32> Don't <00:14.73> you <00:15.14> want <00:15.57> somebody <00:16.09> to <00:16.46> love"#.as_bytes();

    let lrc = Lrc::from_reader(LYRIC).unwrap();

    assert_eq!(
        lrc,
        Lrc(vec![vec![
            (
                TimeTag(Duration::from_secs(0)),
                "When the truth is found to be lies".to_string(),
            ),
            (
                TimeTag(Duration::from_secs(6) + Duration::from_millis(470)),
                "And all the joy within you dies".to_string(),
            ),
            (
                TimeTag(Duration::from_secs(13) + Duration::from_millis(340)),
                "Don't you want somebody to love".to_string(),
            )
        ]
        .into_iter()
        .collect()])
    );
}
