//! Nagrywanie animacji do pliku WebM z obrazem w kodeku AV1 (otwarty i wolny od opłat licencyjnych).
//!
//! Zrzuty ekranu trafiają przez krótką kolejkę do osobnego wątku, który koduje je koderem rav1e
//! i zapisuje w kontenerze WebM. Gdy koder nie nadąża, nadmiarowe klatki są pomijane; każda klatka
//! niesie własny znacznik czasu, więc film zachowuje rzeczywiste tempo.

use std::fs::File;
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use eframe::egui::ColorImage;
use rav1e::prelude::*;

/// Tyle klatek może czekać na koder; kolejne są pomijane.
const QUEUE: usize = 3;

struct Frame {
    image: Arc<ColorImage>,
    /// Lewy górny róg nagrywanego obszaru w pikselach zrzutu.
    origin: [usize; 2],
    /// Co ile pikseli zrzutu przypada jeden piksel filmu (1 = pełna rozdzielczość).
    scale: usize,
    time_ms: u64,
}

pub struct Recorder {
    sender: Option<SyncSender<Frame>>,
    thread: Option<JoinHandle<io::Result<()>>>,
    /// Pełna ścieżka do pliku.
    pub path: PathBuf,
    /// Rozmiar filmu w pikselach.
    pub size: [usize; 2],
    scale: usize,
    min_frame_gap: f64,
    start: Instant,
    last_capture: Option<Instant>,
    encoded: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
}

impl Recorder {
    /// Tworzy plik i uruchamia wątek kodera. `area` to rozmiar nagrywanego obszaru ekranu w pikselach;
    /// film ma rozmiar `area / scale`, zaokrąglony w dół do parzystego (wymóg AV1 4:2:0).
    pub fn start(path: &Path, area: [usize; 2], scale: usize, max_fps: u32) -> io::Result<Self> {
        let scale = scale.max(1);
        let size = [(area[0] / scale) & !1, (area[1] / scale) & !1];
        if size[0] < 16 || size[1] < 16 {
            return Err(io::Error::other("obszar animacji jest za mały"));
        }
        let file = File::create(path)?;
        let (sender, receiver) = mpsc::sync_channel(QUEUE);
        let encoded = Arc::new(AtomicUsize::new(0));
        let thread = thread::Builder::new().name("koder wideo".into()).spawn({
            let encoded = Arc::clone(&encoded);
            move || encode(receiver, file, size, max_fps, &encoded)
        })?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
            path: std::path::absolute(path).unwrap_or_else(|_| path.to_owned()),
            size,
            scale,
            min_frame_gap: 1.0 / max_fps.max(1) as f64,
            start: Instant::now(),
            last_capture: None,
            encoded,
            dropped: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Czy minął już odstęp wynikający z liczby klatek na sekundę.
    pub fn wants_frame(&self, now: Instant) -> bool {
        self.last_capture.is_none_or(|t| (now - t).as_secs_f64() >= self.min_frame_gap * 0.95)
    }

    /// Zwraca funkcję, która przekaże zrzut ekranu koderowi (wywoływaną, gdy zrzut będzie gotowy).
    pub fn capture(&mut self, now: Instant, origin: [usize; 2]) -> impl FnOnce(Arc<ColorImage>) + Send + 'static {
        self.last_capture = Some(now);
        let (sender, scale) = (self.sender.clone(), self.scale);
        let dropped = Arc::clone(&self.dropped);
        let time_ms = (now - self.start).as_millis() as u64;
        move |image| {
            let Some(sender) = sender else { return };
            if let Err(TrySendError::Full(_)) = sender.try_send(Frame { image, origin, scale, time_ms }) {
                dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Liczba klatek zapisanych w pliku i pominiętych, bo koder nie nadążał.
    pub fn stats(&self) -> (usize, usize) {
        (self.encoded.load(Ordering::Relaxed), self.dropped.load(Ordering::Relaxed))
    }

    /// Koduje klatki z kolejki i domyka plik.
    pub fn finish(mut self) -> io::Result<()> {
        self.close()
    }

    fn close(&mut self) -> io::Result<()> {
        self.sender = None;
        match self.thread.take() {
            Some(thread) => thread.join().unwrap_or_else(|_| Err(io::Error::other("koder wideo uległ awarii"))),
            None => Ok(()),
        }
    }
}

impl Drop for Recorder {
    /// Zamknięcie programu w trakcie nagrywania też zostawia poprawny plik.
    fn drop(&mut self) {
        self.close().ok();
    }
}

fn encode(frames: Receiver<Frame>, file: File, [w, h]: [usize; 2], fps: u32, encoded: &AtomicUsize) -> io::Result<()> {
    let enc = EncoderConfig {
        width: w,
        height: h,
        time_base: Rational::new(1, fps.max(1) as u64),
        low_latency: true,
        max_key_frame_interval: 5 * fps.max(1) as u64,
        quantizer: 120,
        speed_settings: SpeedSettings::from_preset(10),
        color_description: Some(ColorDescription {
            color_primaries: ColorPrimaries::BT709,
            transfer_characteristics: TransferCharacteristics::BT709,
            matrix_coefficients: MatrixCoefficients::BT709,
        }),
        pixel_range: PixelRange::Limited,
        tiles: 4,
        ..Default::default()
    };
    let cfg = Config::new().with_encoder_config(enc).with_threads(2);
    let mut ctx: Context<u8> = cfg.new_context().map_err(|e| io::Error::other(format!("koder AV1: {e}")))?;
    let mut webm = WebmWriter::new(BufWriter::new(file), [w, h], &ctx.container_sequence_header())?;
    // Znacznik czasu każdej wysłanej klatki, w kolejności numerów klatek kodera.
    let mut times = Vec::new();
    let (mut y, mut u, mut v) = (vec![0; w * h], vec![0; w * h / 4], vec![0; w * h / 4]);

    for frame in frames {
        rgb_to_yuv420(&frame.image, frame.origin, frame.scale, [w, h], &mut y, &mut u, &mut v);
        let mut f = ctx.new_frame();
        f.planes[0].copy_from_raw_u8(&y, w, 1);
        f.planes[1].copy_from_raw_u8(&u, w / 2, 1);
        f.planes[2].copy_from_raw_u8(&v, w / 2, 1);
        times.push(frame.time_ms);
        ctx.send_frame(f).map_err(encoder_error)?;
        drain(&mut ctx, &mut webm, &times, encoded)?;
    }
    ctx.flush();
    drain(&mut ctx, &mut webm, &times, encoded)?;
    webm.finish()
}

/// Odbiera z kodera wszystkie gotowe pakiety i zapisuje je do pliku.
fn drain(ctx: &mut Context<u8>, webm: &mut WebmWriter, times: &[u64], encoded: &AtomicUsize) -> io::Result<()> {
    loop {
        match ctx.receive_packet() {
            Ok(packet) => {
                let time = times.get(packet.input_frameno as usize).copied().unwrap_or_default();
                webm.write_frame(&packet.data, time, packet.frame_type == FrameType::KEY)?;
                encoded.fetch_add(1, Ordering::Relaxed);
            }
            Err(EncoderStatus::Encoded) => {}
            Err(EncoderStatus::NeedMoreData | EncoderStatus::LimitReached) => return Ok(()),
            Err(e) => return Err(encoder_error(e)),
        }
    }
}

fn encoder_error(e: EncoderStatus) -> io::Error {
    io::Error::other(format!("koder AV1: {e:?}"))
}

/// Konwersja wycinka zrzutu (sRGB) na YUV 4:2:0 według BT.709 w zakresie ograniczonym.
/// Piksel filmu to średnia z kwadratu `scale × scale` pikseli zrzutu; piksele spoza zrzutu
/// (np. po zmniejszeniu okna) są czarne.
#[allow(clippy::too_many_arguments)]
fn rgb_to_yuv420(
    image: &ColorImage,
    [x0, y0]: [usize; 2],
    scale: usize,
    [w, h]: [usize; 2],
    y: &mut [u8],
    u: &mut [u8],
    v: &mut [u8],
) {
    let [iw, ih] = image.size;
    let norm = 1.0 / (scale * scale) as f32;
    let rgb = |x: usize, y: usize| -> [f32; 3] {
        let mut sum = [0.0f32; 3];
        for sy in (y0 + y * scale..).take(scale) {
            for sx in (x0 + x * scale..).take(scale) {
                if sx < iw && sy < ih {
                    let c = image.pixels[sy * iw + sx];
                    sum = [sum[0] + c.r() as f32, sum[1] + c.g() as f32, sum[2] + c.b() as f32];
                }
            }
        }
        sum.map(|s| s * norm)
    };
    for row in (0..h).step_by(2) {
        for col in (0..w).step_by(2) {
            let mut sum = [0.0f32; 3];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let [r, g, b] = rgb(col + dx, row + dy);
                y[(row + dy) * w + col + dx] = (16.0 + 0.1826 * r + 0.6142 * g + 0.0620 * b).round() as u8;
                sum = [sum[0] + r, sum[1] + g, sum[2] + b];
            }
            let [r, g, b] = sum.map(|s| s * 0.25);
            let i = (row / 2) * (w / 2) + col / 2;
            u[i] = (128.0 - 0.1006 * r - 0.3386 * g + 0.4392 * b).round() as u8;
            v[i] = (128.0 + 0.4392 * r - 0.3989 * g - 0.0403 * b).round() as u8;
        }
    }
}

/// Minimalny zapis kontenera WebM (Matroska) z jedną ścieżką wideo AV1.
///
/// Rozmiar segmentu, czas trwania i położenie indeksu (Cues) są uzupełniane po zakończeniu nagrania.
struct WebmWriter {
    out: BufWriter<File>,
    /// Położenie początku danych segmentu w pliku — względem niego liczy się pozycje elementów.
    segment_start: u64,
    /// Położenia w pliku pól uzupełnianych na końcu.
    segment_size_at: u64,
    duration_at: u64,
    cues_position_at: u64,
    cluster: Vec<u8>,
    cluster_time: u64,
    last_time: Option<u64>,
    /// Czas i położenie klastrów zaczynających się od klatki kluczowej (do przewijania).
    cues: Vec<(u64, u64)>,
}

const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114D_9B74;
const INFO: u32 = 0x1549_A966;
const TRACKS: u32 = 0x1654_AE6B;
const CLUSTER: u32 = 0x1F43_B675;
const CUES: u32 = 0x1C53_BB6B;
/// Najdłuższy klaster [ms] — przesunięcie bloku względem klastra mieści się w 16 bitach.
const MAX_CLUSTER_MS: u64 = 30_000;

impl WebmWriter {
    fn new(mut out: BufWriter<File>, [w, h]: [usize; 2], codec_private: &[u8]) -> io::Result<Self> {
        out.write_all(&element(
            0x1A45_DFA3,
            &[
                uint(0x4286, 1),
                uint(0x42F7, 1),
                uint(0x42F2, 4),
                uint(0x42F3, 8),
                string(0x4282, "webm"),
                uint(0x4287, 4),
                uint(0x4285, 2),
            ]
            .concat(),
        ))?;

        // Segment o rozmiarze uzupełnianym na końcu (8-bajtowe pole długości).
        out.write_all(&id(SEGMENT))?;
        let segment_size_at = out.stream_position()?;
        out.write_all(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF])?;
        let segment_start = out.stream_position()?;

        let info = element(
            INFO,
            &[
                uint(0x2A_D7B1, 1_000_000), // znaczniki czasu w milisekundach
                string(0x4D80, "planet_animation"),
                string(0x5741, "planet_animation"),
                float(0x4489, 0.0), // czas trwania — ostatnie pole, uzupełniane na końcu
            ]
            .concat(),
        );
        let tracks = element(
            TRACKS,
            &element(
                0xAE,
                &[
                    uint(0xD7, 1),
                    uint(0x73C5, 1),
                    uint(0x83, 1), // wideo
                    uint(0x9C, 0),
                    string(0x86, "V_AV1"),
                    element(0x63A2, codec_private),
                    element(0xE0, &[uint(0xB0, w as u64), uint(0xBA, h as u64)].concat()),
                ]
                .concat(),
            ),
        );
        // Spis położeń elementów; pozycja indeksu (ostatnie pole) jest uzupełniana na końcu.
        let seek = |target: u32, position: u64| element(0x4DBB, &[element(0x53AB, &id(target)), uint8(0x53AC, position)].concat());
        let seek_head_len = element(SEEK_HEAD, &[seek(INFO, 0), seek(TRACKS, 0), seek(CUES, 0)].concat()).len() as u64;
        let seek_head = element(
            SEEK_HEAD,
            &[seek(INFO, seek_head_len), seek(TRACKS, seek_head_len + info.len() as u64), seek(CUES, 0)].concat(),
        );

        out.write_all(&seek_head)?;
        let cues_position_at = out.stream_position()? - 8;
        out.write_all(&info)?;
        let duration_at = out.stream_position()? - 8;
        out.write_all(&tracks)?;

        Ok(Self {
            out,
            segment_start,
            segment_size_at,
            duration_at,
            cues_position_at,
            cluster: Vec::new(),
            cluster_time: 0,
            last_time: None,
            cues: Vec::new(),
        })
    }

    fn write_frame(&mut self, data: &[u8], time_ms: u64, keyframe: bool) -> io::Result<()> {
        // Czas musi rosnąć, nawet gdy dwie klatki dostały ten sam znacznik.
        let time = self.last_time.map_or(time_ms, |t| time_ms.max(t + 1));
        self.last_time = Some(time);
        if self.cluster.is_empty() || keyframe || time - self.cluster_time > MAX_CLUSTER_MS {
            self.flush_cluster()?;
            self.cluster_time = time;
            if keyframe {
                self.cues.push((time, self.out.stream_position()? - self.segment_start));
            }
        }
        // W Matroska ograniczniki jednostek czasowych (OBU temporal delimiter) są pomijane.
        let data = data.strip_prefix(&[0x12, 0x00]).unwrap_or(data);
        let offset = (time - self.cluster_time) as i16;
        let mut block = vec![0x81]; // ścieżka nr 1
        block.extend_from_slice(&offset.to_be_bytes());
        block.push(if keyframe { 0x80 } else { 0x00 });
        block.extend_from_slice(data);
        self.cluster.extend(element(0xA3, &block));
        Ok(())
    }

    fn flush_cluster(&mut self) -> io::Result<()> {
        if !self.cluster.is_empty() {
            let cluster = element(CLUSTER, &[uint(0xE7, self.cluster_time), std::mem::take(&mut self.cluster)].concat());
            self.out.write_all(&cluster)?;
        }
        Ok(())
    }

    fn finish(mut self) -> io::Result<()> {
        self.flush_cluster()?;
        let cues_position = self.out.stream_position()? - self.segment_start;
        let points: Vec<u8> = self
            .cues
            .iter()
            .flat_map(|&(time, position)| {
                element(0xBB, &[uint(0xB3, time), element(0xB7, &[uint(0xF7, 1), uint(0xF1, position)].concat())].concat())
            })
            .collect();
        self.out.write_all(&element(CUES, &points))?;

        let end = self.out.stream_position()?;
        // Ostatnia klatka trwa umownie 1/30 s.
        let duration = self.last_time.map_or(0.0, |t| t as f64 + 33.0);
        let mut size = (end - self.segment_start).to_be_bytes();
        size[0] = 0x01;
        for (at, bytes) in [
            (self.segment_size_at, size),
            (self.duration_at, duration.to_be_bytes()),
            (self.cues_position_at, cues_position.to_be_bytes()),
        ] {
            self.out.seek(SeekFrom::Start(at))?;
            self.out.write_all(&bytes)?;
        }
        self.out.flush()
    }
}

/// Identyfikator elementu EBML (zawiera już bity długości).
fn id(id: u32) -> Vec<u8> {
    let bytes = id.to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count();
    bytes[skip..].to_vec()
}

/// Rozmiar w kodowaniu EBML o zmiennej długości (najkrótsza postać).
fn vint(n: u64) -> Vec<u8> {
    let len = (1..=8).find(|&len| n < (1u64 << (7 * len)) - 1).expect("element EBML za duży");
    let mut bytes = n.to_be_bytes()[8 - len..].to_vec();
    bytes[0] |= 0x80 >> (len - 1);
    bytes
}

fn element(element_id: u32, payload: &[u8]) -> Vec<u8> {
    [id(element_id), vint(payload.len() as u64), payload.to_vec()].concat()
}

fn uint(element_id: u32, v: u64) -> Vec<u8> {
    let bytes = v.to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count().min(7);
    element(element_id, &bytes[skip..])
}

/// Liczba całkowita zawsze na 8 bajtach — do pól uzupełnianych po fakcie.
fn uint8(element_id: u32, v: u64) -> Vec<u8> {
    element(element_id, &v.to_be_bytes())
}

fn float(element_id: u32, v: f64) -> Vec<u8> {
    element(element_id, &v.to_be_bytes())
}

fn string(element_id: u32, s: &str) -> Vec<u8> {
    element(element_id, s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    #[test]
    fn ebml_sizes_use_shortest_form() {
        assert_eq!(vint(0), [0x80]);
        assert_eq!(vint(126), [0xFE]);
        assert_eq!(vint(127), [0x40, 0x7F]);
        assert_eq!(id(0x1A45_DFA3), [0x1A, 0x45, 0xDF, 0xA3]);
        assert_eq!(uint(0xD7, 1), [0xD7, 0x81, 0x01]);
    }

    /// Nagrywa syntetyczny film (przesuwający się kwadrat), czekając na koder zamiast pomijać klatki.
    pub fn record_test_video(path: &Path, frames: usize, size: [usize; 2], scale: usize) {
        let rec = Recorder::start(path, size, scale, 30).unwrap();
        for i in 0..frames {
            let mut image = ColorImage::filled(size, Color32::from_rgb(4, 5, 12));
            for y in size[1] / 3..size[1] / 2 {
                for x in (8 * i)..(8 * i + 60) {
                    image.pixels[y * size[0] + x % size[0]] = Color32::from_rgb(80, 140, 255);
                }
            }
            let frame = Frame { image: Arc::new(image), origin: [0, 0], scale, time_ms: 33 * i as u64 };
            rec.sender.as_ref().unwrap().send(frame).unwrap();
        }
        rec.finish().unwrap();
    }

    #[test]
    fn writes_webm_file() {
        let path = std::env::temp_dir().join(format!("planet_animation_test_{}.webm", std::process::id()));
        record_test_video(&path, 20, [320, 240], 1);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(&bytes[..4], &[0x1A, 0x45, 0xDF, 0xA3], "brak nagłówka EBML");
        assert!(bytes.windows(5).any(|w| w == b"V_AV1"), "brak ścieżki AV1");
        assert!(bytes.len() > 1000, "plik ma tylko {} bajtów", bytes.len());
    }
}
