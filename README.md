# Układ planetarny 3D

Animacja 3D układu planetarnego z symulacją grawitacji N ciał, napisana w Rust z użyciem
[eframe/egui](https://github.com/emilk/egui).

## Możliwości

- **Fizyka N ciał** — gwiazda i planety przyciągają się wzajemnie; całkowanie metodą leapfrog
  z adaptacyjnym krokiem. Jednostki: AU, lata, masy Słońca.
- **Gotowe układy**: Układ Słoneczny, wewnętrzny Układ Słoneczny, TRAPPIST-1, eksperyment
  z masywnym olbrzymem — oraz własny układ z edycją mas i elementów orbitalnych
  (a, e, i, Ω, ω, ν), dodawaniem i usuwaniem planet.
- **Gaz** (opcjonalnie) — do 1 000 000 losowo rozsianych cząsteczek w dysku wokół gwiazdy.
  Cząsteczki czują grawitację gwiazdy i planet, przyciągają planety i hamują je jak gaz
  w obrębie sfery Hilla. Początkowy profil gęstości dysku Σ(r) można ustawić potęgowo, wzorem
  albo narysować krzywą Béziera (patrz [Profil gęstości dysku](#profil-gęstości-dysku)).
- **Zapis elementów orbitalnych** (opcjonalnie) — oskulacyjne elementy wszystkich planet
  zapisywane co zadany czas do pliku tekstowego (kolumny rozdzielone tabulatorem).
- **Nagrywanie filmu** — zapis animacji do pliku WebM z otwartym kodekiem AV1
  (koder [rav1e](https://github.com/xiph/rav1e)).
- **Wielowątkowość** — fizyka liczy się w osobnym wątku, cząsteczki gazu równolegle na
  wszystkich rdzeniach ([rayon](https://github.com/rayon-rs/rayon)), koder wideo w kolejnym wątku.
- **Płynna lub próbkowana animacja** — gdy komputer nie nadąża, program to sygnalizuje
  i odświeża obraz rzadziej (domyślnie co 1 s); animację można też całkiem wyłączyć,
  a obliczenia trwają dalej.

## Sterowanie

| Akcja | Sposób |
|---|---|
| Obrót kamery | przeciągnij myszą |
| Przybliżenie / oddalenie | kółko myszy |
| Śledzenie ciała | kliknij planetę lub gwiazdę |
| Pauza / wznowienie | spacja |

## Profil gęstości dysku

W sekcji „Gaz” wybiera się, jak gęstość powierzchniowa dysku Σ zależy od odległości od gwiazdy r.
Liczy się tylko kształt profilu — całkowitą masę gazu ustawia się osobno. Wykres pod listą
pokazuje profil od promienia wewnętrznego do zewnętrznego dysku.

- **Potęgowy Σ ∝ r^-p** — p = 1 rozkłada cząsteczki równomiernie w promieniu (domyślnie),
  p = 1,5 odpowiada modelowi „dysku minimalnej masy” (MMSN).
- **Wzór Σ(r)** — dowolne wyrażenie ze zmienną `r` [AU], np. `r^-1 * exp(-(r/20)^2)`
  albo pierścień `exp(-((r-5)/1)^2)`. Dostępne są działania `+ - * / ^`, nawiasy, stałe `pi`, `e`
  i funkcje `exp`, `ln`, `log`, `sqrt`, `abs`, `sin`, `cos`, `tanh`. Liczby można pisać z kropką
  albo przecinkiem.
- **Krzywa Béziera** — profil rysowany myszą na wykresie:

  | Akcja | Sposób |
  |---|---|
  | Przesunięcie punktu (●) | przeciągnij punkt |
  | Zmiana kształtu krzywej | przeciągnij uchwyt (○) |
  | Nowy punkt | dwuklik na wykresie albo przycisk „➕ Dodaj punkt” |
  | Usunięcie punktu | prawy klik na punkcie albo 🗑 przy zaznaczonym punkcie |
  | Wygładzenie | „〰 Wygładź” — automatyczne uchwyty we wszystkich punktach |

  Punkt **gładki** (●) ma uchwyty na jednej prostej, więc krzywa nie ma w nim załamania.
  Po odznaczeniu „gładki” punkt staje się **narożny** (■) i jego uchwyty można ustawiać niezależnie.
  Skrajne punkty przesuwają się tylko w pionie.

W wierszu **„Skala osi”** obie osie wykresu można przełączyć na logarytmiczne:

- **log r** — oś promienia; wygodna, gdy dysk sięga od ułamka AU do dziesiątek AU. W tej skali
  dobierane są też promienie przy rozsiewaniu, więc szczegóły profilu blisko gwiazdy nie giną.
- **log Σ** — oś gęstości, obejmująca zadaną liczbę **dekad** (domyślnie 4, czyli od 10⁻⁴ do 1);
  pokazuje profile zmieniające się o rzędy wielkości. Gęstości mniejsze niż dół zakresu
  krzywa Béziera przyjmuje jako równe dolnej granicy.

Krzywa Béziera leży w wybranych skalach, a przy ich przełączaniu jej punkty zachowują swoje
promienie i gęstości. W skali log-log profil potęgowy jest prostą.

Przycisk **„✏ Edytuj jako krzywą”** zamienia profil potęgowy albo wzór na krzywą Béziera,
którą można dalej poprawiać myszą. Zmieniony profil zaczyna działać po kliknięciu
„🎲 Rozsiej ponownie” (przycisk podświetla się na pomarańczowo).

## Jednostki

| Wielkość | Jednostka | Symbol |
|---|---|---|
| Masa gwiazdy centralnej | masy Słońca | M☉ |
| Masa planety | masy Ziemi | M⊕ |
| Masa dysku gazowego (całkowita) | masy Ziemi | M⊕ |
| Odległości, półosie orbit, promienie dysku | jednostki astronomiczne | AU |
| Czas, odstęp zapisu elementów orbitalnych | lata | — |
| Kąty (i, Ω, ω, ν, grubość dysku) | stopnie | ° |

- 1 M⊕ = 3,003489·10⁻⁶ M☉ (Słońce jest ok. 333 000 razy cięższe od Ziemi),
  1 M♃ (masa Jowisza) = 317,83 M⊕.
- Po najechaniu myszą na „M⊕” przy planecie widać jej masę w masach Jowisza (M♃) i Słońca (M☉).
- Masa gazu to masa **całego** dysku, dzielona po równo między cząsteczki — np. 10 M⊕
  na 300 cząsteczek daje ok. 0,033 M⊕ na cząsteczkę. Zmiana liczby cząsteczek nie zmienia
  więc całkowitego wpływu gazu na planety.
- Wewnętrznie program liczy masy w M☉; w jednostkach AU, lata i M☉ stała grawitacji
  wynosi G = 4π².

## Budowanie

Potrzebny jest [Rust](https://rustup.rs/) w wersji 1.95 lub nowszej (wymaganie biblioteki eframe 0.37).

```sh
cargo build --release
./target/release/planet_animation
```

Program jest pojedynczym plikiem wykonywalnym — nie potrzebuje żadnych dodatkowych plików.

### Linux

Wersja linuksowa działa zarówno w sesji Wayland, jak i X11 (wybiera ją automatycznie).
Wymaga glibc 2.35 lub nowszej (np. Ubuntu 22.04+, Debian 12+).

### Windows (kompilacja skrośna z Linuksa)

```sh
sudo apt install mingw-w64
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

Wynik: `target/x86_64-pc-windows-gnu/release/planet_animation.exe`.

Na Windowsie program rysuje przez **wgpu** (DirectX 12 lub Vulkan), więc działa także bez
sterownika karty graficznej, np. w maszynie wirtualnej (programowy renderer WARP).
Jeśli wgpu nie wystartuje, program sam przełącza się na OpenGL. Na Linuksie rysuje przez OpenGL.

## Testy

```sh
cargo test --release
```

## Struktura

| Plik | Zawartość |
|---|---|
| `src/main.rs` | uruchomienie programu, czcionka z symbolami |
| `src/app.rs` | interfejs użytkownika |
| `src/physics.rs` | symulacja N ciał i gazu |
| `src/engine.rs` | wątek fizyki |
| `src/gas.rs` | rozsiewanie cząsteczek gazu |
| `src/profile.rs` | profil gęstości dysku: wzory, krzywe Béziera, losowanie promieni |
| `src/profile_editor.rs` | edytor profilu gęstości z przeciąganymi punktami i uchwytami |
| `src/logger.rs` | zapis elementów orbitalnych |
| `src/render.rs` | rysowanie sceny 3D |
| `src/system.rs` | gotowe układy planetarne |
| `src/video.rs` | koder AV1 i zapis WebM |
| `src/vec3.rs` | wektory 3D |

## Źródła

Kod powstał na podstawie wiedzy modelu AI, bez sięgania do źródeł w trakcie pisania. Poniżej
wskazano publikacje, z których pochodzą użyte dane i metody.

### Dane astronomiczne

- **Elementy orbit planet (J2000)** — E. M. Standish, *Keplerian Elements for Approximate Positions
  of the Major Planets*, JPL Solar System Dynamics, tabela 1 (lata 1800–2050),
  <https://ssd.jpl.nasa.gov/planets/approx_pos.html>. Argument perycentrum liczony jako ω = ϖ − Ω.
- **Masy planet w masach Ziemi** — D. R. Williams, *Planetary Fact Sheet – Ratio to Earth Values*,
  NASA NSSDCA, <https://nssdc.gsfc.nasa.gov/planetary/factsheet/planet_table_ratio.html>.
- **Stosunki mas Ziemi, Jowisza i Słońca** — ze stosunków GM w układzie stałych IAU:
  B. Luzum i in., *The IAU 2009 system of astronomical constants*, Celest. Mech. Dyn. Astr. 110,
  293 (2011); IAU 2015 Resolution B3.
- **TRAPPIST-1** (masa gwiazdy, masy i półosie planet) — E. Agol i in., *Refining the
  Transit-timing and Photometric Analysis of TRAPPIST-1*, Planet. Sci. J. 2, 1 (2021).
  Odkrycie układu: M. Gillon i in., Nature 533, 221 (2016); Nature 542, 456 (2017).
  Mimośrody, nachylenia i początkowe położenia planet przyjęto umownie.

### Mechanika nieba i metody numeryczne

- **Elementy orbitalne ↔ wektor stanu, sfera Hilla, G = 4π² w jednostkach AU/rok/M☉** —
  C. D. Murray, S. F. Dermott, *Solar System Dynamics*, Cambridge University Press (1999),
  rozdz. 2–3; D. A. Vallado, *Fundamentals of Astrodynamics and Applications*, 4th ed. (2013).
- **Całkowanie leapfrog (KDK, DKD)** — L. Verlet, Phys. Rev. 159, 98 (1967); T. Quinn i in.,
  *Time stepping N-body simulations*, arXiv:astro-ph/9710043 (1997).
- **Adaptacyjny krok czasowy** — P. Hut, J. Makino, S. McMillan, *Building a Better Leapfrog*,
  ApJ 443, L93 (1995).
- **Zmiękczenie potencjału i kryterium kroku z czasu dynamicznego** — S. J. Aarseth,
  *Gravitational N-Body Simulations*, Cambridge University Press (2003).

### Dysk gazowy

- **Dysk minimalnej masy (MMSN, Σ ∝ r⁻¹·⁵)** — S. J. Weidenschilling, Astrophys. Space Sci. 51,
  153 (1977); C. Hayashi, Prog. Theor. Phys. Suppl. 70, 35 (1981).
- **Dyski protoplanetarne (tło)** — P. J. Armitage, *Astrophysics of Planet Formation*, 2nd ed.,
  Cambridge University Press (2020).
- **Opór gazu w sferze Hilla** — model uproszczony, własny; korzysta z pojęcia czasu hamowania,
  np. S. J. Weidenschilling, MNRAS 180, 57 (1977).

### Matematyka i grafika

- **Losowanie metodą odwrotnej dystrybuanty** — L. Devroye, *Non-Uniform Random Variate
  Generation*, Springer (1986).
- **Generator xorshift64** — G. Marsaglia, *Xorshift RNGs*, J. Stat. Softw. 8(14) (2003).
- **Krzywe Béziera** — G. Farin, *Curves and Surfaces for CAGD*, 5th ed., Morgan Kaufmann (2002).
- **Automatyczne uchwyty (styczne Catmulla-Roma)** — E. Catmull, R. Rom, *A Class of Local
  Interpolating Splines*, w: *Computer Aided Geometric Design*, red. R. Barnhill, R. Riesenfeld,
  Academic Press (1974).
- **Algorytm malarza** — M. E. Newell, R. G. Newell, T. L. Sancha, *A Solution to the Hidden
  Surface Problem*, Proc. ACM Annual Conference (1972).

### Wideo

- **Konwersja RGB → YUV** — ITU-R Recommendation BT.709-6 (2015).
- **Kodek AV1** — AOMedia, *AV1 Bitstream & Decoding Process Specification* (2019).
- **Kontener WebM** — RFC 8794 *Extensible Binary Meta Language* (2020); RFC 9559 *Matroska Media
  Container Format Specification* (2024); *AV1 in Matroska/WebM* (IETF CELLAR);
  *WebM Container Guidelines*, <https://www.webmproject.org/docs/container/>.

### Oprogramowanie

[egui/eframe](https://github.com/emilk/egui), [rav1e](https://github.com/xiph/rav1e),
[rayon](https://github.com/rayon-rs/rayon), [DejaVu Fonts](https://dejavu-fonts.github.io/),
[Rust](https://www.rust-lang.org/).

## Autorstwo

Cały kod tego programu napisał model AI Claude (Anthropic) w narzędziu Claude Code,
na podstawie poleceń (promptów) Adama Łacnego, który określał funkcje programu
i kierował jego rozwojem.

## Licencja

Program jest udostępniony na licencji MIT — pełny tekst w pliku [`LICENSE`](LICENSE).

### Licencje zewnętrzne

`assets/symbole.ttf` to podzbiór czcionki DejaVu Sans (symbole astronomiczne ☉ ⊕ ♃ … i znaki ∝ ●),
na licencji Bitstream Vera — pełny tekst w `assets/LICENSE-DejaVu.txt`.
