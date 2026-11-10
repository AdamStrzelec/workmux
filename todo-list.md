# TODO — rozbudowa workmux (fork AdamStrzelec/workmux)

Status: ✅ zrobione · 🔜 następne · ⬜ zaplanowane · 💡 propozycja do decyzji

## 1. Narzędzia od autora workmux (raine)

Decyzja: implementacja natywnie w Rust, bez zewnętrznych binarek. Każde narzędzie dodajemy osobno, krok po kroku.

| # | Narzędzie | Repo | Status | Plan |
|---|-----------|------|--------|------|
| 1 | tmux-agent-usage | raine/tmux-agent-usage | ✅ (Claude, globalnie) | `workmux usage`, nagłówek dashboardu, alerty. Zostały paski postępu |
| 2 | git-surgeon | raine/git-surgeon | ✅ | `workmux hunk` (list, show, stage, unstage, discard, commit, commit-to) gotowe, z blokadą commitów na `main`/`master`/gałąź domyślną. `list --blame` gotowe. `undo`, `amend`, `reword` gotowe (z blokadą na chronionych gałęziach i na wypchniętych commitach). `fold`, `squash`, `move`, `split` gotowe. git-surgeon w całości przeniesiony. Poza zakresem: `update`, `install-skill` |
| 3 | claude-history | raine/claude-history | ⬜ | Przeszukiwanie historii rozmów Claude, powiązanie z worktree/sesją, handoff |
| 4 | consult-llm | raine/consult-llm | ⬜ | Drugi model do review; baza dla pre-merge gate (`/review-panel`) |
| 5 | tmux-tools | raine/tmux-tools | ⬜ | Do przejrzenia, co z tego ma sens w workmux |
| 6 | tmux-file-picker | raine/tmux-file-picker | ⬜ | Wybór plików z poziomu tmux, np. do promptów agentów |
| 7 | tmux-bro | raine/tmux-bro | ⬜ | Do przejrzenia, co z tego ma sens w workmux |

## 2. Kolejność prac (ustalona)

1. Stan, tokeny i alerty ✅
2. Współdzielony stan między użytkownikami Linuksa 🔜 (`workmux team`: snapshoty w katalogu `team.dir`; zostaje publikacja automatyczna, aktywne okno, przełączanie)
3. Bezpieczeństwo: profile, audit log, git-surgeon, polityki git ⬜
4. Reszta: consult-llm, claude-history, porty/dev server, task manager ⬜

## 3. Twoje założenia

### 3.1 Praca zdalna i wielu użytkowników
- Praca głównie na jednym userze, często zdalnie przez SSH na innego usera Linuksa, gdzie narzędzie też będzie zainstalowane.
- Współpraca ze wspólnikami, najczęściej nad tym samym projektem i repo.
- Wspólna orientacja: kto co robi, na jakim branchu i worktree, która sesja tmux jest aktywna.
- Widoczność, czy okno jest aktualnie na ekranie użytkownika (aktywna sesja, tab lub pane) i czy użytkownik jest na nim skupiony.
- Wzajemna widoczność oraz szybkie przełączanie się między sobą.
- Ustalenie: widoczność między osobnymi userami Linuksa wymaga osobnego wyjaśnienia i wspólnego katalogu stanu (np. `/var/lib/workmux/` z grupą do zapisu).

### 3.2 Porty i dev server
- Widok zajętych portów: kto je zajął i co tam działa. ✅ (`workmux ports`, kolumna w dashboardzie, ostrzeżenie o zajętym porcie)
- Gotowe skróty startu projektu (React, Next.js, Storybook itd.).
- Automatyczny wybór wolnego portu.
- Automatyczny port forwarding przy SSH. Ustalenie: konfiguruje to po swojej stronie każdy użytkownik.
- Automatyczne testy jednostkowe w locie.

### 3.3 Zużycie tokenów i alerty
- Zużycie Claude: na razie tylko globalne, bez podziału na użytkowników (ustalone).
- Alert, gdy zostało dużo niewykorzystanych tokenów, a zbliża się koniec okna sesji. Konfigurowalny przez użytkownika (kiedy i jak). ✅ (konfiguracja globalna, reguły `window`, `min_remaining_percent`, `within_minutes`)
- Paski postępu dla sesji (5h) i limitu tygodniowego ✅ (CLI, tmux, nagłówek dashboardu).

### 3.4 Powiadomienia
- Koniec pracy Claude w innym oknie.
- Błędy po zakończeniu np. builda.
- Część już działa przez alerty użycia (macOS + komunikat tmux). Do sprawdzenia: komunikaty tmux przez prawdziwe SSH na innego hosta/usera.

### 3.5 Nadzór nad nietechnicznymi użytkownikami
- Nadzór na żywo, żeby szybko reagować.
- Wykrywanie pracy poza wyznaczonym katalogiem.
- Wykrywanie zmian w plikach, których nie wolno ruszać.
- Problem trybu bypass permissions zamiast auto mode, o którym użytkownik nie wie.
- Ustalenie: da się to testować od razu.

### 3.6 Ograniczenia git
- Agent nie powinien sam mergować do main przed zatwierdzeniem przez kogoś innego.
- Konfigurowalne tryby: autocommit i push do zdalnego PR albo brak autocommitu, żeby osoba techniczna mogła najpierw przejrzeć zmiany.
- Ustawiane w konfiguracji.

### 3.7 Integracja z task managerem (przyszłość)
- TODO → IN_PROGRESS: automatyczne utworzenie worktree.
- IN_REVIEW → DONE albo zmergowany PR: automatyczne usunięcie worktree.
- Własne narzędzie do zarządzania zadaniami.

### 3.8 Klucze SSH i wymiana danych (kolejne kroki)
- Konfiguracja kluczy SSH, żeby część rzeczy robić zdalnie bez pracy na tym samym userze.
- Zachowana komunikacja i wymiana danych między osobami.

## 4. Propozycje (moje), zaakceptowane

1. **Audit log / feed aktywności**: np. „Ania zmieniła .env, Marek zrobił merge do main”. Odpowiedź na problem nietechnicznych użytkowników.
2. **Profile bezpieczeństwa** (`safe`, `review-only`, `trusted`): jednym ustawieniem określają tryb Claude, zakazane ścieżki i politykę git.
3. **Blokada kolizji**: ostrzeżenie, gdy dwie osoby edytują ten sam plik albo branch.
4. **Handoff**: „przekaż mi tę sesję” z kontekstem i przełączeniem okna.
5. **Pre-merge gate**: po PR automatycznie consult-llm `/review-panel` plus testy; merge dopiero po zatwierdzeniu człowieka.
6. **Snapshot / szybki rollback worktree**, gdy agent coś zepsuje.

5. **Hunki w dashboardzie** ❌ odpuszczone (decyzja użytkownika, niepotrzebne w UI): dashboard ma już tryb patch (staging hunk po hunku, split, undo) w `src/command/dashboard/diff_ops.rs`, więc pełny podgląd i staging w TUI byłby duplikatem. Zostaje do decyzji wąski zakres: liczba niezacommitowanych hunków per worktree w liście oraz oznaczenie chronionej gałęzi.

6. **Dziennik błędów i zgłaszanie** (decyzja: tylko lokalnie, bez automatycznego commita i pusha):
   - ✅ `errors.log` w katalogu stanu, `workmux errors`
   - ⬜ `workmux report`: zanonimizowany podgląd i issue w `AdamStrzelec/workmux` po potwierdzeniu, z deduplikacją
   - ⬜ rozróżnienie błędów użytkownika od prawdziwych bugów (teraz każdy błąd komendy trafia do dziennika)
   - 💡 `workmux fix <issue>`: agent naprawia błąd na żądanie, z limitem tokenów; PR tylko do forka, nigdy do upstream

## 5. Ustalenia techniczne

- Rust 2024, clap, serde, ratatui, `ureq` 3 (nowa zależność), `libc::flock` do blokady odświeżania.
- Cache użycia: TTL 55 s, stary cache używany, gdy zapytanie się nie powiedzie.
- Alerty tylko w konfiguracji globalnej, każdy alert odpala się raz na cykl okna.
- tmux `display-message` interpretuje `%` i `#`, więc trzeba je escapować.
- Flow gitowy forka: `main` = lustro upstream, każda zmiana na osobnej gałęzi z `adam`, po zmianach `cargo build && cargo test` i `cargo install --path .`, test w `dev-forge`, push tylko na `origin`.

## 6. Do zrobienia na końcu

- Dokumentacja i CHANGELOG dla sekcji `usage`.
- Weryfikacja powiadomień tmux przez realne SSH.
- Opcjonalnie: zmiana etykiety powiadomień macOS z „Terminal” tylko dla alertów.

## 7. Do przetestowania przez nas (nie sprawdzone przez Claude)

Zasada: wszystko, co zostało dodane, ale nie przetestowane w prawdziwym środowisku, ląduje tutaj.

| Obszar | Co sprawdzić | Dlaczego nie było testowane |
|--------|--------------|-----------------------------|
| `hunk fold` / `move` / `split` | Konflikt w trakcie rebase: czy repo zostaje w czytelnym stanie i czy instrukcja jest zrozumiała | Żaden test nie wywołuje prawdziwego konfliktu |
| `hunk split` | Commit z nowym plikiem (trafia do commita „reszta”, nie da się go wybrać przez `--pick`) | Brak hunka dla nowego pliku, zachowanie tylko opisane |
| `hunk` (całość) | Praca w `dev-forge` z prawdziwym agentem | Testy tylko na tymczasowych repo |
| Zainstalowana binarka | `cargo install --path .` i test nowych komend w `dev-forge` | Binarka w `~/.cargo/bin` jest starsza niż commity |
| `workmux team` | Dwóch prawdziwych użytkowników Linuksa, wspólna grupa, katalog `chmod 2775`, odczyt i zapis plików | Test tylko na jednym użytkowniku i sztucznym pliku |
| `workmux team` | Prywatność: tytuły paneli i wszystkie projekty są widoczne dla wspólników | Decyzja o filtrze jeszcze nie podjęta |
| Alerty użycia | Komunikaty tmux przez prawdziwe SSH na innego hosta/usera | Brak drugiego hosta |
| Alerty użycia | Etykieta powiadomień macOS („Terminal”) | Tylko opcjonalna poprawka |
| `workmux usage` | Odczyt tokena na Linuksie (`~/.claude/.credentials.json`, `CLAUDE_CONFIG_DIR`) | Sprawdzone tylko na macOS (Keychain) |
| Tryb bypass permissions | Wykrywanie i nadzór nad nietechnicznymi użytkownikami | Funkcja jeszcze nie zbudowana |
| Dziennik błędów | Zapis przy prawdziwej panice (hook), rotacja przy >512 KB | Przetestowane tylko trim i serializacja oraz jeden błąd komendy; panika nie została wywołana end-to-end |
