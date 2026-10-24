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
  w obrębie sfery Hilla.
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

Wersja linuksowa korzysta z Waylanda.

### Windows (kompilacja skrośna z Linuksa)

```sh
sudo apt install mingw-w64
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

Wynik: `target/x86_64-pc-windows-gnu/release/planet_animation.exe`.

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
| `src/logger.rs` | zapis elementów orbitalnych |
| `src/render.rs` | rysowanie sceny 3D |
| `src/system.rs` | gotowe układy planetarne |
| `src/video.rs` | koder AV1 i zapis WebM |
| `src/vec3.rs` | wektory 3D |

## Autorstwo

Cały kod tego programu napisał model AI Claude (Anthropic) w narzędziu Claude Code,
na podstawie poleceń (promptów) Adama Łacnego, który określał funkcje programu
i kierował jego rozwojem.

## Licencja

Program jest udostępniony na licencji MIT — pełny tekst w pliku [`LICENSE`](LICENSE).

### Licencje zewnętrzne

`assets/symbole.ttf` to podzbiór czcionki DejaVu Sans (symbole astronomiczne ☉ ⊕ ♃ …),
na licencji Bitstream Vera — pełny tekst w `assets/LICENSE-DejaVu.txt`.
