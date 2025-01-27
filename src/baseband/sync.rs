//! Utilities for detecting the frame synchronization sequence and extracting symbol
//! decoding thresholds from it.

extern crate num;
use num::Zero;

use std;

use collect_slice::CollectSlice;
use moving_avg::MovingAverage;
use static_fir::FIRFilter;

/// Number of samples in the frame sync fingerprint, from first impulse to last, at 48kHz
/// sample rate.
const FINGERPRINT_SAMPS: usize = 231;

/// Number of sync sequences to smooth symbol threshold estimates over.
const SMOOTH_AVG: usize = 4;

/// Continuously cross-correlates input signal with frame sync fingerprint.
pub struct SyncCorrelator {
    /// Fingerprint cross-correlator.
    corr: FIRFilter<SyncFingerprint>,
}

impl SyncCorrelator {
    /// Create a new `SyncCorrelator` with default state.
    pub fn new() -> SyncCorrelator {
        SyncCorrelator {
            corr: FIRFilter::new(),
        }
    }

    /// Cross-correlate with the given sample and return the current correlation power and
    /// signal power within the correlation history.
    pub fn feed(&mut self, sample: f32) -> (f32, f32) {
        (
            self.corr.feed(sample) / FINGERPRINT_SAMPS as f32,
            self.sig_power(),
        )
    }

    fn sig_power(&self) -> f32 {
        self.corr.history_unordered().fold(0.0, |sum, &x| sum + x.powi(2)) /
            FINGERPRINT_SAMPS as f32
    }

    /// Retrieve the sequence of samples that make up the current sync sequence.
    pub fn history(&self) -> [f32; FINGERPRINT_SAMPS] {
        // Since the history is stored as a ring buffer, recreate a continuous signal by
        // concatenating the parts on either side of the split.
        let mut combined = [0.0; FINGERPRINT_SAMPS];
        self.corr.history().cloned().collect_slice_checked(&mut combined[..]);

        combined
    }
}

/// Computes symbol decision thresholds from sync sequences.
pub struct SymbolThresholds {
    /// Smooths estimate for positive symbol threshold.
    psmooth: MovingAverage<f32>,
    /// Smooths estimate for negative symbol threshold.
    nsmooth: MovingAverage<f32>,
}

impl SymbolThresholds {
    /// Create a new `SymbolThresholds` with default state.
    pub fn new() -> Self {
        SymbolThresholds {
            psmooth: MovingAverage::new(SMOOTH_AVG),
            nsmooth: MovingAverage::new(SMOOTH_AVG),
        }
    }

    /// Calculate `(upper, mid, lower)` thresholds for symbol decoding from the given sync
    /// fingerprint samples.
    ///
    /// The first sample should be the sample immediately after the first symbol impulse
    /// in the fingerprint, and the last sample should be the sample immediately after the
    /// final symbol impulse.
    pub fn thresholds(&mut self, sync: &[f32; FINGERPRINT_SAMPS]) -> (f32, f32, f32) {
        let (pavg, navg) = calc_averages(sync);

        let pavg = self.psmooth.feed(pavg);
        let navg = self.nsmooth.feed(navg);

        calc_thresholds(pavg, navg)
    }
}

/// Calculate the average positive (symbol 01) and negative (symbol 11) sample value at
/// each symbol instant in the given samples.
fn calc_averages(samples: &[f32; FINGERPRINT_SAMPS]) -> (f32, f32) {
    /// Indexes of symbol instants for symbol 01.
    const POS: [usize; 10] = [
        0, 10, 20, 30, 50, 60, 90, 100, 150, 170,
    ];

    /// Indexes of symbol instants for symbol 11.
    const NEG: [usize; 13] = [
        40, 70, 80, 110, 120, 130, 140, 160, 180, 190, 200, 210, 220,
    ];

    // First fingerprint symbol has been shifted off, so start at the second one.
    let samples = &samples[9..];

    let pavg = POS.iter().fold(0.0, |s, &idx| s + samples[idx]) / POS.len() as f32;
    let navg = NEG.iter().fold(0.0, |s, &idx| s + samples[idx]) / NEG.len() as f32;

    (pavg, navg)
}

/// Calculate the upper, mid, and lower thresholds for symbol decisions from the given
/// positive and negative sample values.
fn calc_thresholds(pavg: f32, navg: f32) -> (f32, f32, f32) {
    let mthresh = (pavg + navg) / 2.0;
    let pthresh = mthresh + (pavg - mthresh) * (2.0 / 3.0);
    let nthresh = mthresh + (navg - mthresh) * (2.0 / 3.0);

    (pthresh, mthresh, nthresh)
}

/// Compute the sync correlator detection threshold for the given current signal power.
pub fn sync_threshold(sigpower: f32) -> f32 {
    // Empirically-determined power threshold for detecting correlation power with
    // fingerprint, scaled by RMS power of signal under test.
    sigpower.sqrt() * 0.65
}

/// State machine that detects a peak power above an instantaneous threshold. Once the power goes
/// above the threshold, further thresholds are ignored and power is tracked until it peaks.
#[derive(Copy, Clone, Debug)]
pub struct SyncDetector {
    /// Previous maximum power above the threshold, `None` if no power above threshold has been
    /// seen.
    prev: Option<f32>,
}

impl SyncDetector {
    /// Create a new `SyncDetector` in the default state.
    pub fn new() -> SyncDetector {
        SyncDetector {
            prev: None,
        }
    }

    /// Consider the given power related to the given instantaneous power threshold. Return `true`
    /// if the power peaked above threshold in the previous sample and `false` otherwise.
    pub fn detect(&mut self, corrpow: f32, thresh: f32) -> bool {
        match self.prev {
            Some(p) => if corrpow < p {
                return true
            } else {
                self.prev = Some(corrpow);
            },
            None => if corrpow > thresh {
                self.prev = Some(corrpow);
            },
        }

        false
    }
}

// Fingerprint of 24-symbol frame sync pulse waveform.
//
// The first sample represents the impulse instant of the first symbol, and the last
// sample represents the impulse instant of the final symbol.
impl_fir!(SyncFingerprint, f32, FINGERPRINT_SAMPS, [
    1.0,

    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,

    1.0,

    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,

    1.0,

    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,

    1.0,

    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,

    1.0,

    1.0,
    1.0,
    1.0,
    1.0,
    0.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    0.0,
    1.0,
    1.0,
    1.0,
    1.0,

    1.0,

    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,

    1.0,

    1.0,
    1.0,
    1.0,
    1.0,
    0.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    0.0,
    1.0,
    1.0,
    1.0,
    1.0,

    1.0,

    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,

    1.0,

    1.0,
    1.0,
    1.0,
    1.0,
    0.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    0.0,
    1.0,
    1.0,
    1.0,
    1.0,

    1.0,

    1.0,
    1.0,
    1.0,
    1.0,
    0.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    0.0,
    1.0,
    1.0,
    1.0,
    1.0,

    1.0,

    1.0,
    1.0,
    1.0,
    1.0,
    0.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,

    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,

    -1.0,
]);

/// Symbols that make up the frame sync fingerprint.
pub const SYNC_GENERATOR: &'static [u8] = &[
    0b01010101,
    0b01110101,
    0b11110101,
    0b11111111,
    0b01110111,
    0b11111111,
];

#[cfg(test)]
mod test {
    use super::{SyncFingerprint, calc_averages, calc_thresholds, SyncDetector};
    use static_fir::FIRFilter;

    #[test]
    fn test_calc_averages() {
        let (pavg, navg) = calc_averages(&[
                 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0, 42.0,
            -1.0, 42.0,
        ]);

        assert!((pavg - 1.0).abs() < 0.000000001);
        assert!((navg - -1.0).abs() < 0.000000001);
    }

    #[test]
    fn test_calc_thresholds() {
        // Ideal
        let (p, m, n) = calc_thresholds(0.18, -0.18);
        assert!((p - 0.12).abs() < 0.000001);
        assert!((m - 0.0).abs() < 0.000001);
        assert!((n - -0.12).abs() < 0.000001);

        // Scaling
        let (p, m, n) = calc_thresholds(0.072, -0.072);
        assert!((p - 0.048).abs() < 0.000001);
        assert!((m - 0.0).abs() < 0.000001);
        assert!((n - -0.048).abs() < 0.000001);

        // DC bias
        let (p, m, n) = calc_thresholds(0.15, -0.21);
        assert!((p - 0.09).abs() < 0.000001);
        assert!((m - -0.03).abs() < 0.000001);
        assert!((n - -0.15).abs() < 0.000001);

        // Scaling and DC bias
        let (p, m, n) = calc_thresholds(0.042, -0.102);
        assert!((p - 0.018).abs() < 0.000001);
        assert!((m - -0.030).abs() < 0.000001);
        assert!((n - -0.078).abs() < 0.000001);
    }

    #[test]
    fn test_detector() {
        {
            let mut d = SyncDetector::new();
            assert!(!d.detect(0.0, 0.42));
            assert!(!d.detect(0.1, 0.42));
            assert!(!d.detect(0.2, 0.42));
            assert!(!d.detect(0.3, 0.42));
            assert!(!d.detect(0.4, 0.42));
            assert!(!d.detect(0.1, 0.42));
            assert!(!d.detect(-0.5, 0.42));
            assert!(!d.detect(0.43, 0.42));
            assert!(!d.detect(0.43, 0.42));
            assert!(!d.detect(0.43, 0.42));
            assert!(!d.detect(0.5, 0.42));
            assert!(!d.detect(0.6, 0.42));
            assert!(!d.detect(0.7, 0.42));
            assert!(d.detect(0.6, 0.42));
        }
        {
            let mut d = SyncDetector::new();
            assert!(!d.detect(0.0, 0.1));
            assert!(!d.detect(0.1, 0.1));
            assert!(!d.detect(0.2, 0.1));
            assert!(!d.detect(0.3, 0.5));
            assert!(d.detect(0.2, 0.5));
        }
    }

    #[test]
    fn test_corr_impulses() {
        let samps = [
            1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            -1.0,
        ];

        let mut corr = FIRFilter::<SyncFingerprint>::new();

        let val = samps.iter().fold(0.0, |_, &s| {
            corr.feed(s)
        });

        assert!((val - 24.0).abs() < 1.0e-12);
    }

    #[test]
    fn test_corr_self() {
        // Verify result of correlating fingerprint with pulse-shaped version. Result
        // verified with simple python script.
        let samps = [
            0.1800000071525574,
            0.1978198289871216,
            0.2099689990282059,
            0.2165211588144302,
            0.2179928719997406,
            0.2152651846408844,
            0.2094739675521851,
            0.2018802613019943,
            0.1937347948551178,
            0.1861512959003448,
            0.1800000071525574,
            0.1752818375825882,
            0.1726729571819305,
            0.1721030473709106,
            0.1732143014669418,
            0.1754311621189117,
            0.1780535280704498,
            0.1803612411022186,
            0.1817191243171692,
            0.1816691458225250,
            0.1800000071525574,
            0.1769981086254120,
            0.1729207485914230,
            0.1683343797922134,
            0.1639688909053802,
            0.1606296300888062,
            0.1590951979160309,
            0.1600108742713928,
            0.1637911200523376,
            0.1705398857593536,
            0.1800000071525574,
            0.1920341849327087,
            0.2051814645528793,
            0.2181062698364258,
            0.2292642593383789,
            0.2370275855064392,
            0.2398276031017303,
            0.2363019436597824,
            0.2254338264465332,
            0.2066690623760223,
            0.1800000071525574,
            0.1451723426580429,
            0.1041970252990723,
            0.0588545575737953,
            0.0113610159605742,
            -0.0357748419046402,
            -0.0799224898219109,
            -0.1185150444507599,
            -0.1492338180541992,
            -0.1701772212982178,
            -0.1800000071525574,
            -0.1782248616218567,
            -0.1647633463144302,
            -0.1402871310710907,
            -0.1060971990227699,
            -0.0640321969985962,
            -0.0163415372371674,
            0.0344656258821487,
            0.0857832729816437,
            0.1350704431533813,
            0.1800000071525574,
            0.2185133844614029,
            0.2493223547935486,
            0.2713244855403900,
            0.2838969528675079,
            0.2868903875350952,
            0.2805941700935364,
            0.2656784951686859,
            0.2431197315454483,
            0.2141158133745193,
            0.1800000071525574,
            0.1420873105525970,
            0.1020096987485886,
            0.0610143989324570,
            0.0201945826411247,
            -0.0195273663848639,
            -0.0573977828025818,
            -0.0928147062659264,
            -0.1253007054328918,
            -0.1544706970453262,
            -0.1800000071525574,
            -0.2013290226459503,
            -0.2182873338460922,
            -0.2306725829839706,
            -0.2383142113685608,
            -0.2410729974508286,
            -0.2388465106487274,
            -0.2315795421600342,
            -0.2192782014608383,
            -0.2020259797573090,
            -0.1800000071525574,
            -0.1532173305749893,
            -0.1221901029348373,
            -0.0875204429030418,
            -0.0499646067619324,
            -0.0104162562638521,
            0.0301185827702284,
            0.0705537423491478,
            0.1097602546215057,
            0.1466066390275955,
            0.1800000071525574,
            0.2078066021203995,
            0.2303429692983627,
            0.2469505518674850,
            0.2571383714675903,
            0.2606004178524017,
            0.2572255432605743,
            0.2471018731594086,
            0.2305136024951935,
            0.2079318165779114,
            0.1800000071525574,
            0.1482946127653122,
            0.1128981560468674,
            0.0747754648327827,
            0.0349638536572456,
            -0.0054574888199568,
            -0.0453989915549755,
            -0.0837926641106606,
            -0.1196240484714508,
            -0.1519640088081360,
            -0.1800000071525574,
            -0.2038477063179016,
            -0.2221774905920029,
            -0.2346453368663788,
            -0.2411370575428009,
            -0.2417868077754974,
            -0.2369892895221710,
            -0.2274002730846405,
            -0.2139248698949814,
            -0.1976900100708008,
            -0.1800000071525574,
            -0.1620066016912460,
            -0.1452740132808685,
            -0.1312751173973083,
            -0.1213541179895401,
            -0.1166122332215309,
            -0.1177986711263657,
            -0.1252161413431168,
            -0.1386523246765137,
            -0.1573466956615448,
            -0.1800000071525574,
            -0.2037107646465302,
            -0.2275322079658508,
            -0.2491591274738312,
            -0.2662219703197479,
            -0.2764815092086792,
            -0.2780291438102722,
            -0.2694737911224365,
            -0.2500989139080048,
            -0.2199728488922119,
            -0.1800000071525574,
            -0.1307839453220367,
            -0.0759941488504410,
            -0.0187530945986509,
            0.0374600626528263,
            0.0890670716762543,
            0.1326695233583450,
            0.1653263270854950,
            0.1848054677248001,
            0.1897873282432556,
            0.1800000071525574,
            0.1562021374702454,
            0.1205543875694275,
            0.0758518949151039,
            0.0255915038287640,
            -0.0262900032103062,
            -0.0757186934351921,
            -0.1188024878501892,
            -0.1521399319171906,
            -0.1730929315090179,
            -0.1800000071525574,
            -0.1720438003540039,
            -0.1499400287866592,
            -0.1154771521687508,
            -0.0714401230216026,
            -0.0213812477886677,
            0.0306713040918112,
            0.0805418044328690,
            0.1242440789937973,
            0.1582981497049332,
            0.1800000071525574,
            0.1877036094665527,
            0.1805642098188400,
            0.1590872108936310,
            0.1248394548892975,
            0.0802870690822601,
            0.0285635516047478,
            -0.0268088988959789,
            -0.0822194814682007,
            -0.1342625766992569,
            -0.1800000071525574,
            -0.2164000272750854,
            -0.2429153919219971,
            -0.2590250074863434,
            -0.2650501430034637,
            -0.2620624899864197,
            -0.2517325878143311,
            -0.2361349165439606,
            -0.2175311595201492,
            -0.1981521546840668,
            -0.1800000071525574,
            -0.1642665565013885,
            -0.1526461988687515,
            -0.1457018554210663,
            -0.1434255987405777,
            -0.1453046500682831,
            -0.1504340618848801,
            -0.1576614528894424,
            -0.1657462716102600,
            -0.1735156029462814,
            -0.1800000071525574,
            -0.1837651729583740,
            -0.1854088008403778,
            -0.1850511729717255,
            -0.1831333339214325,
            -0.1803399324417114,
            -0.1774928718805313,
            -0.1754284054040909,
            -0.1748728752136230,
            -0.1763309538364410,
            -0.1800000071525574,
            -0.1861512809991837,
            -0.1937348246574402,
            -0.2018802165985107,
            -0.2094739824533463,
            -0.2152651846408844,
            -0.2179928719997406,
            -0.2165211737155914,
            -0.2099690437316895,
            -0.1978198438882828,
            -0.1800000071525574,
        ];

        let mut corr = FIRFilter::<SyncFingerprint>::new();

        let val = samps.iter().fold(0.0, |_, &s| {
            corr.feed(s)
        });

        assert!((val - 37.710987).abs() < 0.00001);
    }
}
