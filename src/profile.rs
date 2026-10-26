//! Początkowy profil gęstości powierzchniowej dysku gazu Σ(r).
//!
//! Profil można podać na trzy sposoby: potęgowo (Σ ∝ r⁻ᵖ), wzorem z promieniem `r` [AU]
//! albo krzywą Béziera rysowaną we współrzędnych znormalizowanych: x = 0 to promień wewnętrzny
//! dysku, x = 1 zewnętrzny, y ∈ [0, 1] to gęstość względna. Liczy się tylko kształt profilu —
//! całkowitą masę gazu ustawia się osobno.

/// Liczba przedziałów, na które dzielimy dysk przy losowaniu promieni i rysowaniu profilu.
pub const TABLE_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileKind {
    PowerLaw,
    Formula,
    Bezier,
}

impl ProfileKind {
    pub const ALL: [ProfileKind; 3] = [ProfileKind::PowerLaw, ProfileKind::Formula, ProfileKind::Bezier];

    pub fn label(self) -> &'static str {
        match self {
            ProfileKind::PowerLaw => "Potęgowy Σ ∝ r^-p",
            ProfileKind::Formula => "Wzór Σ(r)",
            ProfileKind::Bezier => "Krzywa Béziera",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DensityProfile {
    pub kind: ProfileKind,
    /// Wykładnik p w Σ ∝ r⁻ᵖ. Dla p = 1 promienie cząsteczek są rozłożone równomiernie.
    pub exponent: f64,
    /// Wzór Σ(r), np. `r^-1 * exp(-r/20)`.
    pub formula: String,
    pub curve: BezierCurve,
}

impl Default for DensityProfile {
    fn default() -> Self {
        Self {
            kind: ProfileKind::PowerLaw,
            exponent: 1.0,
            formula: "r^-1 * exp(-(r/20)^2)".into(),
            curve: BezierCurve::default(),
        }
    }
}

impl DensityProfile {
    /// Gęstość w `TABLE_SIZE + 1` równo rozłożonych promieniach od `lo` do `hi` (wartości ≥ 0).
    pub fn table(&self, lo: f64, hi: f64) -> Result<Vec<f64>, String> {
        let r_at = |k: usize| lo + (hi - lo) * k as f64 / TABLE_SIZE as f64;
        let values: Vec<f64> = match self.kind {
            ProfileKind::PowerLaw => (0..=TABLE_SIZE).map(|k| r_at(k).powf(-self.exponent)).collect(),
            ProfileKind::Formula => {
                let expr = Expr::parse(&self.formula)?;
                (0..=TABLE_SIZE).map(|k| expr.eval(r_at(k))).collect()
            }
            ProfileKind::Bezier => (0..=TABLE_SIZE).map(|k| self.curve.eval(k as f64 / TABLE_SIZE as f64)).collect(),
        };
        let values: Vec<f64> = values.into_iter().map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 }).collect();
        if values.iter().all(|&v| v == 0.0) {
            return Err("Gęstość wynosi zero w całym dysku".into());
        }
        Ok(values)
    }

    /// Losowanie promieni cząsteczek zgodnie z profilem: masa pierścienia ∝ Σ(r)·r·dr.
    pub fn radius_sampler(&self, lo: f64, hi: f64) -> Result<RadiusSampler, String> {
        let sigma = self.table(lo, hi)?;
        let dr = (hi - lo) / TABLE_SIZE as f64;
        let mut cumulative = Vec::with_capacity(TABLE_SIZE + 1);
        let mut total = 0.0;
        cumulative.push(0.0);
        for k in 0..TABLE_SIZE {
            let (r0, r1) = (lo + dr * k as f64, lo + dr * (k + 1) as f64);
            total += 0.5 * (sigma[k] * r0 + sigma[k + 1] * r1) * dr;
            cumulative.push(total);
        }
        if total <= 0.0 {
            return Err("Gęstość wynosi zero w całym dysku".into());
        }
        Ok(RadiusSampler { lo, dr, cumulative })
    }

    /// Zamienia bieżący profil na krzywą Béziera o `points` punktach, gotową do ręcznej edycji.
    pub fn convert_to_curve(&mut self, lo: f64, hi: f64, points: usize) -> Result<(), String> {
        let table = self.table(lo, hi)?;
        let max = table.iter().copied().fold(0.0, f64::max);
        let anchors = (0..points)
            .map(|i| {
                let x = i as f64 / (points - 1) as f64;
                let k = (x * TABLE_SIZE as f64).round() as usize;
                Point::new(x, table[k] / max)
            })
            .collect::<Vec<_>>();
        self.curve = BezierCurve::through(&anchors);
        self.kind = ProfileKind::Bezier;
        Ok(())
    }
}

/// Odwrotna dystrybuanta rozkładu promieni.
pub struct RadiusSampler {
    lo: f64,
    dr: f64,
    cumulative: Vec<f64>,
}

impl RadiusSampler {
    /// Promień dla `u` z przedziału [0, 1).
    pub fn radius(&self, u: f64) -> f64 {
        let target = u * self.cumulative[self.cumulative.len() - 1];
        let k = self.cumulative.partition_point(|&c| c <= target).clamp(1, self.cumulative.len() - 1) - 1;
        let (c0, c1) = (self.cumulative[k], self.cumulative[k + 1]);
        let frac = if c1 > c0 { (target - c0) / (c1 - c0) } else { 0.5 };
        self.lo + self.dr * (k as f64 + frac)
    }
}

// ---------------------------------------------------------------------------------------------
// Krzywa Béziera

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }

    fn scale(self, s: f64) -> Point {
        Point::new(self.x * s, self.y * s)
    }

    fn length(self) -> f64 {
        self.x.hypot(self.y)
    }
}

/// Punkt węzłowy krzywej z dwoma uchwytami (przesunięciami względem punktu).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub pos: Point,
    pub left: Point,
    pub right: Point,
    /// Gładki punkt ma uchwyty na jednej prostej — krzywa nie ma w nim załamania.
    pub smooth: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Krzywa złożona z odcinków sześciennych między kolejnymi punktami węzłowymi.
///
/// Niezmienniki: punkty są posortowane po x, pierwszy ma x = 0, ostatni x = 1, wszystkie y ∈ [0, 1].
/// Uchwyty sięgają w poziomie co najwyżej do połowy odcinka, więc x(t) każdego odcinka jest
/// monotoniczne — krzywa jest funkcją promienia.
#[derive(Clone, Debug, PartialEq)]
pub struct BezierCurve {
    pub anchors: Vec<Anchor>,
}

impl Default for BezierCurve {
    fn default() -> Self {
        Self::through(&[Point::new(0.0, 0.25), Point::new(0.35, 1.0), Point::new(1.0, 0.15)])
    }
}

impl BezierCurve {
    /// Gładka krzywa przez podane punkty (x od 0 do 1).
    pub fn through(points: &[Point]) -> Self {
        let anchors = points
            .iter()
            .map(|&p| Anchor { pos: p, left: Point::ZERO, right: Point::ZERO, smooth: true })
            .collect();
        let mut curve = Self { anchors };
        curve.smooth_all();
        curve
    }

    /// Ustawia uchwyty wszystkich punktów automatycznie, tak by krzywa była gładka.
    pub fn smooth_all(&mut self) {
        for i in 0..self.anchors.len() {
            self.auto_handles(i);
        }
    }

    /// Automatyczne uchwyty punktu `i`: styczna jak w splajnie Catmulla-Roma, długość ⅓ odcinka.
    pub fn auto_handles(&mut self, i: usize) {
        let n = self.anchors.len();
        let p = self.anchors[i].pos;
        let prev = self.anchors[i.saturating_sub(1)].pos;
        let next = self.anchors[(i + 1).min(n - 1)].pos;
        let dx = next.x - prev.x;
        let slope = if dx > 0.0 { (next.y - prev.y) / dx } else { 0.0 };
        let a = &mut self.anchors[i];
        a.smooth = true;
        let (dl, dr) = (p.x - prev.x, next.x - p.x);
        a.left = Point::new(-dl / 3.0, -slope * dl / 3.0);
        a.right = Point::new(dr / 3.0, slope * dr / 3.0);
        self.clamp_handles(i);
    }

    /// Przesuwa punkt `i`; skrajne punkty poruszają się tylko w pionie.
    pub fn move_anchor(&mut self, i: usize, to: Point) {
        const GAP: f64 = 1e-3;
        let last = self.anchors.len() - 1;
        let x = match i {
            0 => 0.0,
            _ if i == last => 1.0,
            _ => to.x.clamp(self.anchors[i - 1].pos.x + GAP, self.anchors[i + 1].pos.x - GAP),
        };
        self.anchors[i].pos = Point::new(x, to.y.clamp(0.0, 1.0));
        for j in i.saturating_sub(1)..=(i + 1).min(last) {
            self.clamp_handles(j);
        }
    }

    /// Ustawia uchwyt punktu `i` (przesunięcie względem punktu). W gładkim punkcie drugi
    /// uchwyt obraca się razem z nim, zachowując swoją długość.
    pub fn move_handle(&mut self, i: usize, side: Side, offset: Point) {
        let a = &mut self.anchors[i];
        let (moved, other) = match side {
            Side::Left => (&mut a.left, &mut a.right),
            Side::Right => (&mut a.right, &mut a.left),
        };
        *moved = offset;
        let len = offset.length();
        if a.smooth && len > 0.0 {
            *other = offset.scale(-other.length() / len);
        }
        self.clamp_handles(i);
    }

    /// Wstawia nowy punkt na krzywej w miejscu `x` (albo w punkcie `at`, jeśli podany y)
    /// i zwraca jego indeks.
    pub fn insert(&mut self, at: Point) -> Option<usize> {
        let x = at.x.clamp(0.0, 1.0);
        let i = self.anchors.partition_point(|a| a.pos.x < x);
        if i == 0 || i == self.anchors.len() || (self.anchors[i].pos.x - x).abs() < 1e-3 {
            return None;
        }
        if (self.anchors[i - 1].pos.x - x).abs() < 1e-3 {
            return None;
        }
        let anchor = Anchor { pos: Point::new(x, at.y.clamp(0.0, 1.0)), left: Point::ZERO, right: Point::ZERO, smooth: true };
        self.anchors.insert(i, anchor);
        self.auto_handles(i);
        self.clamp_handles(i - 1);
        self.clamp_handles(i + 1);
        Some(i)
    }

    /// Usuwa punkt `i` (skrajnych nie da się usunąć).
    pub fn remove(&mut self, i: usize) -> bool {
        if i == 0 || i + 1 >= self.anchors.len() {
            return false;
        }
        self.anchors.remove(i);
        self.clamp_handles(i - 1);
        self.clamp_handles(i);
        true
    }

    /// Ogranicza uchwyty punktu `i`: uchwyty skierowane do środka odcinka, sięgające najwyżej
    /// do jego połowy. Uchwyt za długi w poziomie jest skracany z zachowaniem kierunku.
    fn clamp_handles(&mut self, i: usize) {
        let n = self.anchors.len();
        let x = self.anchors[i].pos.x;
        let half_left = if i > 0 { (x - self.anchors[i - 1].pos.x) / 2.0 } else { 0.0 };
        let half_right = if i + 1 < n { (self.anchors[i + 1].pos.x - x) / 2.0 } else { 0.0 };
        let a = &mut self.anchors[i];
        a.left = clamp_handle(a.left, -half_left);
        a.right = clamp_handle(a.right, half_right);
    }

    /// Punkty kontrolne odcinka `seg` (od punktu `seg` do `seg + 1`).
    pub fn segment(&self, seg: usize) -> [Point; 4] {
        let (a, b) = (&self.anchors[seg], &self.anchors[seg + 1]);
        [a.pos, a.pos.add(a.right), b.pos.add(b.left), b.pos]
    }

    /// Wartość krzywej (≥ 0) w punkcie x ∈ [0, 1].
    pub fn eval(&self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        let seg = self.anchors.partition_point(|a| a.pos.x <= x).clamp(1, self.anchors.len() - 1) - 1;
        let p = self.segment(seg);
        // x(t) jest monotoniczne, więc wystarczy bisekcja.
        let (mut t0, mut t1) = (0.0, 1.0);
        for _ in 0..40 {
            let t = 0.5 * (t0 + t1);
            if bezier(p, t).x < x {
                t0 = t;
            } else {
                t1 = t;
            }
        }
        bezier(p, 0.5 * (t0 + t1)).y.max(0.0)
    }

    /// Łamana przybliżająca krzywą, do rysowania.
    pub fn polyline(&self, per_segment: usize) -> Vec<Point> {
        let mut pts = vec![self.anchors[0].pos];
        for seg in 0..self.anchors.len() - 1 {
            let p = self.segment(seg);
            pts.extend((1..=per_segment).map(|k| bezier(p, k as f64 / per_segment as f64)));
        }
        pts
    }
}

/// Ogranicza uchwyt tak, by jego składowa x miała znak `limit` i |x| ≤ |limit|.
fn clamp_handle(h: Point, limit: f64) -> Point {
    if limit == 0.0 {
        return Point::ZERO;
    }
    let h = if h.x * limit < 0.0 { Point::new(0.0, h.y) } else { h };
    if h.x.abs() > limit.abs() { h.scale(limit.abs() / h.x.abs()) } else { h }
}

fn bezier(p: [Point; 4], t: f64) -> Point {
    let s = 1.0 - t;
    let (a, b, c, d) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
    Point::new(
        a * p[0].x + b * p[1].x + c * p[2].x + d * p[3].x,
        a * p[0].y + b * p[1].y + c * p[2].y + d * p[3].y,
    )
}

// ---------------------------------------------------------------------------------------------
// Wzory

/// Funkcja jednej zmiennej dostępna we wzorach.
type Function = fn(f64) -> f64;

/// Wyrażenie arytmetyczne z jedną zmienną `r`.
#[derive(Clone, Debug)]
pub enum Expr {
    Num(f64),
    R,
    Neg(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    Call(Function, Box<Expr>),
}

const FUNCTIONS: [(&str, Function); 8] = [
    ("exp", f64::exp),
    ("ln", f64::ln),
    ("log", f64::log10),
    ("sqrt", f64::sqrt),
    ("abs", f64::abs),
    ("sin", f64::sin),
    ("cos", f64::cos),
    ("tanh", f64::tanh),
];

/// Nazwy funkcji dostępnych we wzorach (do podpowiedzi w interfejsie).
pub fn function_names() -> impl Iterator<Item = &'static str> {
    FUNCTIONS.iter().map(|(name, _)| *name)
}

impl Expr {
    /// Składnia: liczby (także z przecinkiem dziesiętnym), `r`, `pi`, `e`, działania `+ - * /`,
    /// potęga `^` (lub `**`), nawiasy i funkcje z `FUNCTIONS`, np. `exp(-r/10)`.
    pub fn parse(text: &str) -> Result<Expr, String> {
        let tokens = tokenize(text)?;
        let mut parser = Parser { tokens, pos: 0 };
        let expr = parser.sum()?;
        match parser.peek() {
            None => Ok(expr),
            Some(t) => Err(format!("Nieoczekiwany znak: {t}")),
        }
    }

    pub fn eval(&self, r: f64) -> f64 {
        match self {
            Expr::Num(v) => *v,
            Expr::R => r,
            Expr::Neg(a) => -a.eval(r),
            Expr::Call(f, a) => f(a.eval(r)),
            Expr::Bin(op, a, b) => {
                let (a, b) = (a.eval(r), b.eval(r));
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    _ => a.powf(b),
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Num(f64),
    Name(String),
    Op(char),
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Num(v) => write!(f, "{v}"),
            Token::Name(n) => write!(f, "{n}"),
            Token::Op(c) => write!(f, "{c}"),
        }
    }
}

fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' || c == ',' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == ',') {
                i += 1;
            }
            // Wykładnik dziesiętny: 1e-3, 2.5E4.
            if i + 1 < chars.len()
                && (chars[i] == 'e' || chars[i] == 'E')
                && (chars[i + 1].is_ascii_digit() || (matches!(chars[i + 1], '+' | '-') && i + 2 < chars.len() && chars[i + 2].is_ascii_digit()))
            {
                i += 2;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let s: String = chars[start..i].iter().map(|&c| if c == ',' { '.' } else { c }).collect();
            tokens.push(Token::Num(s.parse().map_err(|_| format!("Błędna liczba: {s}"))?));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            tokens.push(Token::Name(chars[start..i].iter().collect::<String>().to_lowercase()));
        } else if c == '*' && chars.get(i + 1) == Some(&'*') {
            tokens.push(Token::Op('^'));
            i += 2;
        } else if "+-*/^()".contains(c) {
            tokens.push(Token::Op(c));
            i += 1;
        } else {
            return Err(format!("Nieznany znak: {c}"));
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn eat(&mut self, op: char) -> bool {
        if self.peek() == Some(&Token::Op(op)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Result<Expr, String> {
        let mut left = self.product()?;
        loop {
            let op = if self.eat('+') {
                '+'
            } else if self.eat('-') {
                '-'
            } else {
                return Ok(left);
            };
            left = Expr::Bin(op, Box::new(left), Box::new(self.product()?));
        }
    }

    fn product(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            let op = if self.eat('*') {
                '*'
            } else if self.eat('/') {
                '/'
            } else {
                return Ok(left);
            };
            left = Expr::Bin(op, Box::new(left), Box::new(self.unary()?));
        }
    }

    /// Minus jednoargumentowy wiąże słabiej niż potęga: -r^2 = -(r^2).
    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat('-') {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }

    /// Potęga jest prawostronnie łączna, a wykładnik może mieć znak: r^-1, 2^3^2 = 2^9.
    fn power(&mut self) -> Result<Expr, String> {
        let base = self.atom()?;
        if self.eat('^') {
            return Ok(Expr::Bin('^', Box::new(base), Box::new(self.unary()?)));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Expr, String> {
        let token = self.peek().cloned().ok_or("Wzór urywa się — brakuje liczby lub r")?;
        self.pos += 1;
        match token {
            Token::Num(v) => Ok(Expr::Num(v)),
            Token::Op('(') => {
                let inner = self.sum()?;
                if !self.eat(')') {
                    return Err("Brakuje nawiasu zamykającego )".into());
                }
                Ok(inner)
            }
            Token::Name(name) => match name.as_str() {
                "r" => Ok(Expr::R),
                "pi" => Ok(Expr::Num(std::f64::consts::PI)),
                "e" => Ok(Expr::Num(std::f64::consts::E)),
                _ => {
                    let f = FUNCTIONS
                        .iter()
                        .find(|(n, _)| *n == name)
                        .map(|(_, f)| *f)
                        .ok_or_else(|| format!("Nieznana nazwa: {name} (zmienna to r)"))?;
                    if !self.eat('(') {
                        return Err(format!("Po {name} potrzebny jest nawias ("));
                    }
                    let arg = self.sum()?;
                    if !self.eat(')') {
                        return Err("Brakuje nawiasu zamykającego )".into());
                    }
                    Ok(Expr::Call(f, Box::new(arg)))
                }
            },
            Token::Op(c) => Err(format!("Nieoczekiwany znak: {c}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(text: &str, r: f64) -> f64 {
        Expr::parse(text).unwrap().eval(r)
    }

    #[test]
    fn formulas_follow_usual_precedence() {
        assert_eq!(eval("1 + 2 * 3", 0.0), 7.0);
        assert_eq!(eval("-r^2", 3.0), -9.0);
        assert_eq!(eval("2^3^2", 0.0), 512.0);
        assert_eq!(eval("r^-1", 4.0), 0.25);
        assert_eq!(eval("r**2 / (1 + 1)", 4.0), 8.0);
        assert_eq!(eval("2,5 * r", 2.0), 5.0);
        assert_eq!(eval("1e-1 * 3E1", 0.0), 3.0);
        assert!((eval("exp(-r/10) * sqrt(r)", 10.0) - (-1f64).exp() * 10f64.sqrt()).abs() < 1e-12);
        assert!((eval("2 * pi * e", 0.0) - 2.0 * std::f64::consts::PI * std::f64::consts::E).abs() < 1e-12);
    }

    #[test]
    fn formula_errors_are_reported() {
        for bad in ["", "r +", "(r", "foo(r)", "exp r", "r # 2", "2 3"] {
            assert!(Expr::parse(bad).is_err(), "{bad:?} powinien być błędny");
        }
    }

    #[test]
    fn curve_passes_through_anchors_and_stays_a_function() {
        let mut c = BezierCurve::through(&[Point::new(0.0, 0.2), Point::new(0.3, 0.9), Point::new(1.0, 0.4)]);
        // Uchwyt przeciągnięty daleko poza odcinek zostaje skrócony.
        c.move_handle(1, Side::Right, Point::new(5.0, 2.0));
        for seg in 0..2 {
            let p = c.segment(seg);
            assert!(p[0].x <= p[1].x && p[1].x <= p[2].x && p[2].x <= p[3].x, "odcinek {seg}: {p:?}");
        }
        for a in &c.anchors {
            assert!((c.eval(a.pos.x) - a.pos.y).abs() < 1e-6);
        }
        // Gładki punkt: uchwyty na jednej prostej, w przeciwnych kierunkach.
        let a = c.anchors[1];
        assert!((a.left.x * a.right.y - a.left.y * a.right.x).abs() < 1e-12);
        assert!(a.left.x <= 0.0 && a.right.x >= 0.0);
    }

    #[test]
    fn anchors_can_be_inserted_moved_and_removed() {
        let mut c = BezierCurve::default();
        let n = c.anchors.len();
        let i = c.insert(Point::new(0.7, 0.5)).unwrap();
        assert_eq!(c.anchors.len(), n + 1);
        assert!(c.anchors.windows(2).all(|w| w[0].pos.x < w[1].pos.x));
        c.move_anchor(i, Point::new(-3.0, 7.0));
        assert!(c.anchors[i].pos.x > c.anchors[i - 1].pos.x && c.anchors[i].pos.y == 1.0);
        c.move_anchor(0, Point::new(0.5, 0.5));
        assert_eq!(c.anchors[0].pos.x, 0.0, "skrajny punkt porusza się tylko w pionie");
        assert!(!c.remove(0));
        assert!(c.remove(i));
        assert_eq!(c.anchors.len(), n);
    }

    #[test]
    fn sampled_radii_follow_profile() {
        let mean = |profile: &DensityProfile| {
            let sampler = profile.radius_sampler(1.0, 3.0).unwrap();
            (0..10_000).map(|k| sampler.radius((k as f64 + 0.5) / 10_000.0)).sum::<f64>() / 10_000.0
        };
        // Σ ∝ 1/r: masa pierścienia nie zależy od r, więc średni promień to środek dysku.
        let flat_in_r = DensityProfile::default();
        assert!((mean(&flat_in_r) - 2.0).abs() < 1e-3);
        // Wąski pierścień w r = 2,5.
        let ring = DensityProfile { kind: ProfileKind::Formula, formula: "exp(-((r-2.5)/0.05)^2)".into(), ..Default::default() };
        assert!((mean(&ring) - 2.5).abs() < 0.01);
        let empty = DensityProfile { kind: ProfileKind::Formula, formula: "0 * r".into(), ..Default::default() };
        assert!(empty.radius_sampler(1.0, 3.0).is_err());
    }

    #[test]
    fn profile_converts_to_editable_curve() {
        let mut p = DensityProfile { exponent: 1.5, ..Default::default() };
        p.convert_to_curve(1.0, 4.0, 6).unwrap();
        assert_eq!(p.kind, ProfileKind::Bezier);
        assert_eq!(p.curve.anchors.len(), 6);
        assert!((p.curve.eval(0.0) - 1.0).abs() < 1e-9);
        assert!((p.curve.eval(1.0) - 4f64.powf(-1.5)).abs() < 1e-3);
    }
}
