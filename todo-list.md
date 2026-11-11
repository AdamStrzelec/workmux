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
2. Współdzielony stan między użytkownikami Linuksa 🔜 (`workmux team`: snapshoty w katalogu `team.dir`; ✅ publikacja automatyczna z dashboardu i filtr prywatności; zostaje aktywne okno i przełączanie)
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

### 3.9 Jak działa `workmux team` (opis szczegółowy)

**Cel:** wspólnicy na jednej maszynie (osobni userzy Linuksa) widzą, kto nad czym pracuje, bez demona i bez serwera.

**Mechanizm**
1. Każdy user ma własny plik `<team.dir>/<user>.json` (np. `/var/lib/workmux/ania.json`). Zapisuje tylko swój plik, nikt nie edytuje cudzych.
2. Plik to migawka: user, host, czas publikacji (`published_at`) i lista agentów. Dla agenta: projekt, worktree, branch, status, sesja i okno tmux, tytuł panelu, czas ostatniej aktywności.
3. Zapis jest atomowy (plik tymczasowy + `rename`), więc czytający nigdy nie zobaczy połowy pliku. Prawa 0664: właściciel i grupa mogą czytać i pisać.
4. Nazwa pliku jest czyszczona (`../x` staje się `_x`), więc nazwa usera nie wyjdzie poza katalog.
5. Odczyt (`workmux team`) czyta wszystkie `*.json` w katalogu, pomija uszkodzone i ukryte, sortuje po userze i wypisuje tabelę USER / PROJECT / BRANCH / STATUS / WINDOW / ACTIVE.
6. Migawka starsza niż `stale_after_secs` (domyślnie 300 s) jest oznaczona `(stale 12m)`. Zwykle znaczy to, że user ma zamknięty dashboard.

**Kiedy migawka jest publikowana**
- Ręcznie: `workmux team` publikuje twoją migawkę i pokazuje tabelę. `--no-publish` tylko czyta, `--json` daje JSON.
- Automatycznie: otwarty dashboard co 60 s publikuje w wątku w tle (`auto_publish`, domyślnie włączone). Bez ustawionego `team.dir` nic się nie dzieje.
- Bez dashboardu migawka się nie odświeża, a po 5 minutach jest `stale`.

**Konfiguracja (tylko globalna, sekcja `team`)**
- `dir`: wspólny katalog; bez niego funkcja jest wyłączona.
- `name`: nazwa widoczna dla innych (domyślnie `$USER`).
- `stale_after_secs`: próg nieaktualności.
- `auto_publish`: publikacja z dashboardu (domyślnie `true`).
- `share_titles`: czy publikować tytuły paneli (domyślnie `false`, bo często zawierają streszczenie promptu).
- `exclude_projects`: projekty, które nigdy nie trafią do migawki.

**Przygotowanie katalogu (raz, root):** utworzyć grupę (np. `workmux`), dodać do niej userów, `mkdir /var/lib/workmux`, `chgrp workmux`, `chmod 2775` (bit setgid, żeby nowe pliki dziedziczyły grupę).

**Prywatność i granice**
- Widoczne dla grupy: nazwy projektów, branche, statusy, nazwy okien, czasy. Tytuły tylko po włączeniu `share_titles`.
- Nie ma uwierzytelniania: każdy z grupy może nadpisać cudzy plik (prawa grupowe). To zaufanie między wspólnikami, nie zabezpieczenie.
- Nie przesyła treści kodu, promptów ani rozmów.

**Czego jeszcze nie ma:** widoczność aktywnego okna (czy user patrzy na dany pane), szybkie przełączanie między userami, publikacja natychmiast po zmianie statusu (jest co 60 s), kolumna team w dashboardzie.

### 3.10 Jak działa `workmux audit` (opis szczegółowy)

**Co robi:** każda komenda zmieniająca stan zostawia wpis w lokalnym `audit.log`. Odpowiada na pytanie „kto, co i kiedy zrobił", np. kto zrobił merge albo usunął worktree.

**Krok po kroku:**
1. `main()` po wykonaniu komendy (sukces lub błąd) wywołuje `audit::record_command`. Worker `_deferred-cleanup` jest pomijany.
2. Komenda jest klasyfikowana po pierwszym argumencie niebędącym flagą: add, merge, remove (alias `rm`), close, rename, rebase, send, run, set-base, resurrect, sync-files oraz `hunk <sub>` poza `list` i `show`. Pozostałe (np. `list`, `status`) nie są zapisywane.
3. Wpis to JSON w jednej linii: czas, user (z `team.name` albo `$USER`), akcja, projekt, branch, ok. Wartości argumentów nigdy nie są zapisywane, bo mogą zawierać prompty i ścieżki.
4. Plik leży w katalogu stanu usera, poza gitem. Po przekroczeniu 1 MB najstarsza połowa jest obcinana.
5. Podgląd: `workmux audit [--json] [--last N]` (domyślnie 30).

**Granice:** log jest lokalny (każdy user ma swój), nie chroni przed ręcznym `git`, a zapis błędu nigdy nie psuje właściwej komendy.

**Udostępnianie zespołowi (`team.share_audit`):** po włączeniu `team: {dir: ..., share_audit: true}` każdy wpis trafia dodatkowo do `<team.dir>/audit-<user>.log` (JSON-lines, tryb 0664, obcinanie przy 1 MB jak lokalnie). `workmux audit --team [--json] [--last N]` czyta wszystkie takie pliki i pokazuje wpisy wszystkich użytkowników od najstarszego. Domyślnie wyłączone. Projekty z `team.exclude_projects` nie są udostępniane. Ograniczenia: brak uwierzytelniania, każdy z prawem zapisu do katalogu może dopisać lub zmienić cudzy plik i pole `user` jest deklarowane przez piszącego; nazwy projektów i branchy są widoczne dla wszystkich z dostępem do katalogu; udostępnianie dotyczy tylko nowych wpisów (stare lokalne się nie kopiują); błąd zapisu do katalogu jest ignorowany.

### 3.11 Jak działa `workmux guard` (opis szczegółowy)

**Co robi:** sprawdza, czy w którymś worktree zmieniono plik z listy chronionych (np. `.env`, klucze). Odpowiada na obawę, że agent z bypass permissions ruszy pliki, których nie wolno.

**Krok po kroku:**
1. Lista wzorców jest w globalnym configu (`~/.config/workmux/config.yaml`), sekcja `guard.protected_paths`. Tylko globalnie, projekt nie może jej wyłączyć. Przykład:
   ```yaml
   guard:
     protected_paths: [".env*", "secrets/**", "*.pem"]
   ```
2. Wzorzec bez `/` pasuje do nazwy pliku na dowolnej głębokości (`.env*` złapie `app/.env.local`). Wzorzec ze `/` pasuje do ścieżki od roota repo (`secrets/**` złapie `secrets/k.pem`, ale nie `x/secrets/k`). Błędny wzorzec jest pomijany bez błędu.
3. `workmux guard` bierze listę worktree (bez głównego checkoutu) i bazę porównania (domyślny branch, lokalny lub `origin/`).
4. Dla każdego worktree zbiera zmienione pliki z trzech źródeł: `git diff --name-only baza...HEAD` (commity), `git status --porcelain` (zmiany niezacommitowane i nieśledzone, z obsługą zmiany nazwy).
5. Lista jest filtrowana wzorcami. Wynik: branch i pliki (lub `--json`: mapa branch → pliki). Kod wyjścia 2, gdy coś znaleziono, 0 gdy czysto.
6. Gdy lista wzorców jest pusta, komenda tylko o tym informuje.

**Granice:**
- Tylko wykrywa: nie blokuje merge, nie cofa zmian, nie zapisuje się w audit logu.
- Sprawdza stan w chwili uruchomienia, nie ciągle; agent może zmienić plik i cofnąć go przed sprawdzeniem.
- Git uruchamiany przez utwardzony konstruktor workmux (bez hooków repo).

**Następne kroki:** widok w dashboardzie, współdzielenie przez katalog team.

### 3.12 Blokada `workmux merge` przy zmianie chronionego pliku (opis szczegółowy)

Cel: agent z bypass permissions (lub nietechniczny partner) nie wmerguje do głównego branchu zmian w `.env` czy `secrets/`, zanim ktoś to obejrzy.

Krok po kroku:
1. Używa tej samej listy `guard.protected_paths` z globalnego configu co `workmux guard` (3.11).
2. `workmux merge` przed właściwym mergem (po rozwiązaniu nazwy worktree, przed hookami pre-merge i czyszczeniem) woła `guard::ensure_merge_allowed`.
3. Funkcja bierze worktree, liczy bazę względem docelowego branchu (`--into` albo `main_branch` z configu) i zbiera zmienione pliki: commity od bazy, zmiany niezacommitowane i nieśledzone.
4. Jeśli któryś plik pasuje do wzorca, merge kończy się błędem `merge blocked: protected files changed:` z listą plików. Nic nie jest zmergowane ani usunięte.
5. Pusta lista wzorców: zachowanie bez zmian, zero kosztu.
6. Świadome obejście: `workmux merge --allow-protected`.

Ograniczenia (uczciwie):
- `--allow-protected` może podać też agent. To bramka ostrzegawcza, nie twarde zabezpieczenie. Twarde wymaga profili `safe`/`review-only` (sekcja 4).
- Baza to `--into` albo `main_branch`, nie automatycznie wykryty branch bazowy z `workmux add`. Przy innej bazie lista może być szersza (bezpieczniej, nie węziej).
- Nie obejmuje merge zdalnych workspace'ów ani merge przez sandbox/RPC.
- Zablokowany merge zapisuje się w audit logu jako `merge blocked (protected files)` (ok=false), obok zwykłego wpisu `merge` (ok=false).

### 3.13 Profile bezpieczeństwa `safety_profile` (opis szczegółowy)

Cel: jednym ustawieniem ograniczyć, co workmux danego użytkownika w ogóle może zrobić. Dla nietechnicznych partnerów albo agentów z bypass permissions.

Konfiguracja (globalny config danego użytkownika Linuxa, `~/.config/workmux/config.yaml`):
```yaml
safety_profile: review-only   # trusted (domyślnie) | safe | review-only
```

Krok po kroku:
1. Przy starcie, przed wykonaniem jakiejkolwiek komendy, `main` czyta profil z globalnego configu i woła `profile::check` z pełnym argv.
2. Komenda jest klasyfikowana tą samą funkcją co w audit logu (`audit::action_label`). Komendy tylko do odczytu (`list`, `status`, `hunk list/show` itd.) nigdy nie są blokowane.
3. Reguły:
   - `trusted`: bez ograniczeń (zachowanie jak dotąd).
   - `safe`: blokuje `hunk discard|undo|amend|reword|fold|squash|move|split` (kasowanie pracy i przepisywanie historii) oraz `merge --allow-protected`. Zwykły `merge`, `remove`, `rebase`, `hunk stage|unstage|commit|commit-to` działają.
   - `review-only`: wszystko z `safe` plus blokada `merge`, `remove` (też `rm`) i `rebase`. Można pracować (`add`, `send`, `run`, `hunk commit`), ale zmergować i usunąć worktree musi ktoś z profilem `trusted`.
4. Zablokowana komenda kończy się błędem `'<komenda>' is not allowed under safety profile '<profil>'` i wpisem w audit logu `blocked by profile <profil>: <komenda>` (ok=false).
5. Błędna wartość profilu w configu: cały config jest odrzucony przez parser, więc użyje się domyślnego `trusted` (patrz ograniczenia).

Ograniczenia (uczciwie):
- To NIE jest granica bezpieczeństwa wobec samego użytkownika: kto może edytować swój `~/.config/workmux/config.yaml`, zmieni sobie profil. Daje ochronę przed pomyłką i przed agentem, który nie dotyka configu, nie przed świadomym obejściem.
- Agent z bypass permissions może wywołać gołego `git` (`git merge`, `git push`) z pominięciem workmux. Profil nie ogranicza surowego gita. Twarde wymuszenie wymaga uprawnień systemowych (np. protected branch na GitHubie, hook po stronie serwera, konfiguracja pliku przez roota).
- Literówka lub błąd składni w configu daje cichy powrót do `trusted` (fail-open). Do rozważenia: fail-closed.
- Blokuje tylko komendy workmux, nie `hunk` poza listą wyżej, i nie obejmuje zdalnych workspace'ów po stronie węzła (`node`).

Uzupełnienie (fail-closed): gdy globalny config istnieje, ale się nie parsuje (literówka, zła struktura), obowiązuje najostrzejszy profil `review-only`, a odmowa pokazuje przyczynę i błąd parsowania. Brak pliku to nadal `trusted`. Komendy tylko do odczytu działają bez zmian, więc plik można naprawić ręcznie. Skutek uboczny: literówka w dowolnej sekcji configu (nie tylko profilu) blokuje `merge`, `remove` i `rebase` aż do naprawy.

### 3.14 Wykrywanie kolizji `workmux collisions` (opis szczegółowy)

Cel: zanim dwie osoby (albo dwa agenty) skończą pracę i pokłócą się w merge, pokazać, że dotykają tego samego.

Krok po kroku:
1. Dla każdego worktree repozytorium (bez głównego checkoutu) komenda zbiera zmienione pliki tak samo jak `guard`: commity od bazy plus zmiany niezacommitowane i nieśledzone.
2. Pliki zmienione w co najmniej dwóch różnych branchach wypisuje jako `file <plik>: <branch1>, <branch2>`.
3. Jeśli skonfigurowany jest katalog `team.dir`, czyta migawki innych użytkowników i wypisuje `branch <branch>: <user1>, <user2>` dla branchy tego samego projektu, na których pracuje agent co najmniej dwóch różnych użytkowników. Migawki starsze niż `team.stale_after_secs` są pomijane. Bez `team.dir` ta część jest cicho pominięta.
4. `--json` daje `{"files": {...}, "branches": {...}}`. Kod wyjścia 2, gdy znaleziono jakąkolwiek kolizję, 0 gdy nic.

Ograniczenia (uczciwie):
- Tylko wykrywa, nie blokuje ani niczego nie zapisuje do audit logu.
- Zmiany plików widzi tylko w worktree tego samego repozytorium na tej maszynie. Migawki team zawierają branche, nie pliki (celowo, ze względu na prywatność), więc kolizji na poziomie plików między użytkownikami nie widać.
- Nazwa projektu to nazwa katalogu repo, więc dwa różne repozytoria o tej samej nazwie zlałyby się w jedno.
- Wymaga ręcznego uruchomienia; nie ma jeszcze ostrzeżenia w dashboardzie. Przy `workmux add` jest ostrzeżenie o branchu (patrz niżej).

Uzupełnienie (ostrzeżenie przy `add`): `workmux add <branch>` (bez `--dry-run`) czyta migawki z `team.dir` i jeśli inny użytkownik ma świeży wpis agenta na tym samym branchu tego samego projektu, wypisuje na stderr `warning: branch '<b>' already has an agent of: <user>`. Tylko ostrzega, nigdy nie blokuje i nie trafia do audit logu. Bez `team.dir` albo przy błędzie odczytu jest cicho. Sprawdza tylko zadany jawnie branch (nie `--auto-name`, `--pr`, ani nazw z promptu), a pliki między użytkownikami nadal niewidoczne.

### 3.15 Snapshot i rollback worktree `workmux snapshot` (opis szczegółowy)

1. `workmux snapshot save [etykieta]` w worktree zapisuje cały stan plików: śledzone, zmienione i nieśledzone (bez ignorowanych przez `.gitignore`).
2. Zapis idzie przez tymczasowy indeks, więc prawdziwy indeks, branch i historia nie są ruszane. Powstaje commit trzymany przez ref `refs/workmux/snapshots/<branch>/<czas>-<etykieta>` (lokalnie, nie wypycha się go z `git push` domyślnie).
3. `workmux snapshot list [--json]` pokazuje snapshoty bieżącego brancha, najnowsze pierwsze.
4. `workmux snapshot restore <id>` najpierw zapisuje obecny stan jako `before-restore` (żeby cofnięcie było możliwe), potem przywraca pliki ze snapshotu (`git read-tree --reset -u`) i zdejmuje je z indeksu.
5. Restore trafia do audit logu jako `snapshot restore`; save i list nie.

Ograniczenia: przywraca tylko pliki, nie cofa commitów ani nie przesuwa brancha. Nieśledzone pliki utworzone po snapshocie zostają (nic nie jest kasowane, poza plikami śledzonymi, których nie było w snapshocie). Snapshot jest ręczny, bez automatu i bez czyszczenia starych. Git z pominięciem hooków (jak reszta narzędzi).

Uzupełnienie (czyszczenie): `workmux snapshot prune [--keep N]` (domyślnie 20) usuwa refy najstarszych snapshotów bieżącego brancha, zostawiając N najnowszych. Kasuje tylko refy; obiekty zbierze `git gc`. Wpis `snapshot prune` trafia do audit logu. Ręcznie.

Uzupełnienie (auto-czyszczenie): opcja `guard.snapshot_keep: N` w globalnym configu (N > 0) powoduje, że po każdym `snapshot save` oraz po automatycznym snapshocie przed `send` zostaje N najnowszych snapshotów brancha, starsze refy są kasowane. Błąd czyszczenia jest ignorowany (zapis nie zawodzi). Nie obejmuje snapshotu `before-restore` robionego przez `restore`, więc ten może chwilowo przekroczyć limit do następnego zapisu. Bez opcji albo przy 0 nic nie jest kasowane. Czyszczenie nie trafia do audit logu.

### 3.16 Handoff `workmux handoff` (opis szczegółowy)

1. W worktree `workmux handoff` drukuje notatkę markdown dla osoby lub agenta przejmującego pracę.
2. Zawartość: nazwa brancha, branch bazowy, commity od bazy (`git log base..HEAD`), zmiany niezacommitowane (`git status --porcelain`), wszystkie zmienione pliki (commity + niezacommitowane + nieśledzone) oraz liczba snapshotów brancha.
3. `--json` daje te same dane maszynowo.
4. Tylko czyta; nic nie zapisuje i nie trafia do audit logu. Git leci przez utwardzony konstruktor (bez hooków).

Ograniczenia: bez treści rozmowy z agentem i bez przełączania okna tmux (czeka na claude-history). Notatka jest do skopiowania ręcznie.

### 3.17 Bramka przed mergem `guard.review_command` (opis szczegółowy)

1. W globalnym configu `guard.review_command` to dowolna komenda shell, np. wywołanie consult-llm jako review-panel. Domyślnie pusta (brak bramki).
2. `workmux merge` uruchamia ją (`sh -c`, katalog = worktree) po sprawdzeniu `protected_paths` i przed właściwym mergem. Na stdin dostaje diff `base...HEAD`; w środowisku `WORKMUX_BRANCH` i `WORKMUX_TARGET`. Wyjście komendy widać w terminalu.
3. Kod wyjścia ≠ 0 blokuje merge i zapisuje `merge blocked (review failed)` w audit logu. `--skip-review` pomija bramkę.
4. Sam workmux nie woła żadnego LLM: komendę wybiera użytkownik. Do consult-llm wystarczy ją tu wpisać, gdy zostanie zintegrowane (sekcja 4).

Ograniczenia: bez limitu czasu i bez limitu rozmiaru diffu (cały diff idzie na stdin). To bramka ostrzegawcza, nie granica bezpieczeństwa: użytkownik albo agent z dostępem do configu lub surowego `git merge` ją omija. Działa tylko w `workmux merge` z CLI; merge z dashboardu/sidebaru i zdalny (`homecoming`) jej nie sprawdzają.

### 3.18 Automatyczny snapshot przed `workmux send` (opis szczegółowy)

1. Opcja `guard.snapshot_before_send: true` w globalnym configu (domyślnie wyłączona).
2. Gdy włączona, `workmux send` przed wysłaniem promptu do lokalnego agenta zapisuje snapshot jego worktree z etykietą `before-send` (patrz 3.15) i wypisuje `snapshot saved: <id>` na stderr.
3. Błąd snapshotu tylko ostrzega (`warning: snapshot before send failed`) i prompt i tak idzie, żeby nie blokować pracy.
4. Odtwarzanie: `workmux snapshot list` i `restore <id>` w tym worktree.

Ograniczenia: tylko `send` do lokalnego agenta; zdalni agenci, `workmux run`, prompty wpisane ręcznie w tmux i akcje z dashboardu nie robią snapshotu. Brak czyszczenia starych snapshotów, więc przy częstym `send` refy się namnażają. Na dużym repo każdy `send` trwa dłużej (skan plików).

## 4. Propozycje (moje), zaakceptowane

1. **Audit log / feed aktywności**: np. „Ania zmieniła .env, Marek zrobił merge do main”. Odpowiedź na problem nietechnicznych użytkowników.
   - ✅ lokalny `audit.log` (stan usera, poza gitem) + `workmux audit [--json] [--last N]`: user, komenda (add/merge/remove/close/rename/rebase/send/run/set-base/resurrect/sync-files, `hunk` poza list/show), projekt, branch, ok/FAILED. Bez wartości argumentów.
   - ✅ `workmux guard [--json]`: globalna konfiguracja `guard.protected_paths` (wzorce glob; bez `/` dopasowanie po nazwie pliku na dowolnej głębokości, np. `.env*`; z `/` po ścieżce od roota repo, np. `secrets/**`). Dla każdego worktree pokazuje zmienione chronione pliki (commity względem bazy + niezacommitowane + nieśledzone). Kod wyjścia 2, gdy coś znaleziono. Tylko wykrywa, niczego nie blokuje ani nie cofa.
   - ✅ `workmux merge` odmawia, gdy branch zmienił chronione pliki; obejście `--allow-protected` (patrz 3.12)
   - ✅ wpisy w audit logu: `guard violation` (gdy `workmux guard` coś znajdzie) i `merge blocked (protected files)`; tylko etykieta, bez nazw plików
   - ✅ współdzielenie logu przez katalog team (`team.share_audit`, `workmux audit --team`, patrz 3.10)
   - ✅ wskaźnik w nagłówku dashboardu: `⚠ guard N · collisions M` (worktree z chronionymi plikami; pliki w 2+ worktree plus branche 2+ użytkowników), odświeżany co 60 s w tle, znika gdy zero. Tylko liczby, bez nazw plików; szczegóły: `workmux guard`, `workmux collisions`
   - ⬜ widok listy zdarzeń audit w dashboardzie
2. **Profile bezpieczeństwa** (`safe`, `review-only`, `trusted`): jednym ustawieniem określają tryb Claude, zakazane ścieżki i politykę git.
   - ✅ `safety_profile` w globalnym configu: ograniczenia komend workmux (patrz 3.13)
   - ✅ fail-closed: nieparsowalny global config daje `review-only` (patrz 3.13)
   - ⬜ tryb Claude per profil, wymuszenie poza workmux (protected branch, hooki serwera)
3. **Blokada kolizji**: ostrzeżenie, gdy dwie osoby edytują ten sam plik albo branch.
   - ✅ `workmux collisions [--json]`: pliki zmienione w 2+ worktree oraz branche z agentami 2+ użytkowników (patrz 3.14)
   - ✅ ostrzeżenie w dashboardzie (licznik w nagłówku) i przy `workmux add`
   - ⬜ wpis w audit logu, kolizje plików między użytkownikami (wymaga listy plików w migawce)
4. **Handoff**: „przekaż mi tę sesję” z kontekstem i przełączeniem okna.
   - ✅ `workmux handoff [--json]`: notatka przekazania bieżącego worktree (branch, baza, commity od bazy, zmiany niezacommitowane, wszystkie zmienione pliki, liczba snapshotów) — patrz 3.16
   - ⬜ przełączenie okna tmux, dołączenie ostatnich promptów/historii rozmowy (wymaga claude-history)
5. **Pre-merge gate**: po PR automatycznie consult-llm `/review-panel` plus testy; merge dopiero po zatwierdzeniu człowieka.
6. **Snapshot / szybki rollback worktree**, gdy agent coś zepsuje.
   - ✅ `workmux snapshot save [etykieta] | list [--json] | restore <id>` (patrz 3.15)
   - ✅ automatyczny snapshot przed `workmux send` (opcja `guard.snapshot_before_send`, patrz 3.18)
   - ✅ `workmux snapshot prune [--keep N]` (patrz 3.15)
   - ⬜ snapshot przed `run`, automatyczne czyszczenie, snapshot commitów (reset brancha)

5. **Hunki w dashboardzie** ❌ odpuszczone (decyzja użytkownika, niepotrzebne w UI): dashboard ma już tryb patch (staging hunk po hunku, split, undo) w `src/command/dashboard/diff_ops.rs`, więc pełny podgląd i staging w TUI byłby duplikatem. Zostaje do decyzji wąski zakres: liczba niezacommitowanych hunków per worktree w liście oraz oznaczenie chronionej gałęzi.

6. **Dziennik błędów i zgłaszanie** (decyzja: tylko lokalnie, bez automatycznego commita i pusha):
   - ✅ `errors.log` w katalogu stanu, `workmux errors`
   - ✅ wskaźnik `⚠ N (workmux errors)` w nagłówku dashboardu (tylko paniki, znika po `workmux errors`)
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
| `workmux team` | Prywatność: tytuły paneli i wszystkie projekty są widoczne dla wspólników | Filtr gotowy: tytuły domyślnie ukryte, `exclude_projects`; sprawdzić na żywo |
| `workmux team` | Auto-publikacja z dashboardu co 60 s (wątek w tle) | Niesprawdzone w prawdziwym tmux; wymaga ustawionego `team.dir` |
| Alerty użycia | Komunikaty tmux przez prawdziwe SSH na innego hosta/usera | Brak drugiego hosta |
| Alerty użycia | Etykieta powiadomień macOS („Terminal”) | Tylko opcjonalna poprawka |
| `workmux usage` | Odczyt tokena na Linuksie (`~/.claude/.credentials.json`, `CLAUDE_CONFIG_DIR`) | Sprawdzone tylko na macOS (Keychain) |
| Tryb bypass permissions | Wykrywanie i nadzór nad nietechnicznymi użytkownikami | Funkcja jeszcze nie zbudowana |
| Dziennik błędów | Zapis przy prawdziwej panice (hook), rotacja przy >512 KB | Przetestowane tylko trim i serializacja oraz jeden błąd komendy; panika nie została wywołana end-to-end |
| Wskaźnik błędów w dashboardzie | Wygląd `⚠ N` w nagłówku, zniknięcie po `workmux errors`, zachowanie na wąskim terminalu | Nie uruchamiałem dashboardu w tmux; brak testu jednostkowego (zależy od zmiennej środowiskowej) |
| `workmux guard` | Ustaw w globalnym configu `guard: {protected_paths: [".env*"]}`, zmień `.env` w worktree (też bez commita) i uruchom `workmux guard`: ma wypisać branch i plik, kod wyjścia 2. Sprawdź też wzorzec ze `/` | Testy pokrywają tylko dopasowanie wzorców, nie git na prawdziwym repo |
| `workmux merge` + guard | Z `guard.protected_paths: [".env*"]` zmień `.env` w worktree i uruchom `workmux merge`: ma odmówić z listą plików. Powtórz z `--allow-protected`: ma przejść. Sprawdź też merge bez zmian chronionych | Brak testu na prawdziwym repo, tylko kompilacja i istniejące testy |
| `safety_profile` | W globalnym configu ustaw `safety_profile: review-only`. Sprawdź: `workmux merge x`, `workmux remove x`, `workmux hunk discard` mają odmówić; `workmux add`, `workmux list`, `workmux hunk commit` mają działać. Potem `safe` (blokuje tylko rewrite/discard i `merge --allow-protected`). Zajrzyj do `workmux audit` | Testy pokrywają tylko klasyfikację komend, nie przepływ na prawdziwym configu |
| `workmux collisions` | Zmień ten sam plik w dwóch worktree i uruchom: ma wypisać `file ...`, kod wyjścia 2. Z `team.dir` i dwoma użytkownikami na tym samym branchu ma wypisać `branch ...` | Testy pokrywają tylko logikę łączenia; na realnym repo sprawdzony tylko przypadek bez kolizji |
| Ostrzeżenie przy `workmux add` | Z `team.dir` i świeżą migawką drugiego użytkownika z agentem na branchu X uruchom `workmux add X`: ma pojawić się `warning: branch 'X' already has an agent of: ...` i worktree dalej powstaje | Tylko testy jednostkowe logiki; ścieżek `--auto-name`/`--pr`/headless/RPC nie sprawdzałem na żywo |
| `workmux audit` | Po `workmux merge`/`rm`/`hunk commit` w prawdziwym repo: wpis w `audit.log` (user, projekt, branch, ok). Czy `rm` i komendy z aliasami łapią się poprawnie | Testy sprawdzają tylko klasyfikację komend, nie zapis w prawdziwym stanie |
| `team.share_audit` | Dwóch użytkowników na jednej maszynie z tym samym `team.dir` i `share_audit: true`: każdy robi komendę zmieniającą stan, potem `workmux audit --team`. Sprawdź prawa plików `audit-*.log` (grupa musi móc czytać) i że `exclude_projects` ukrywa projekt | Sprawdzone tylko lokalnie jednym użytkownikiem na tymczasowym katalogu; nie na dwóch kontach Linux ani z prawdziwą grupą |
| `guard.review_command` | Wpisz w global config `guard: {review_command: "cat >/dev/null; exit 1"}` i `workmux merge` ma odmówić; potem `exit 0` i `--skip-review`. Sprawdź prawdziwą komendę z LLM, duży diff, merge z dashboardu (nie powinien jej uruchamiać) | Sprawdzone tylko testem jednostkowym wywołania komendy; nie e2e |
| `guard.snapshot_before_send` | Włącz w global config, wyślij `workmux send <agent> \"...\"` do lokalnego agenta; sprawdź `snapshot saved: ...` i `snapshot list`. Zmierz czas na dużym repo | Niesprawdzone z prawdziwym agentem; tylko build i testy |
| `snapshot prune` | W worktree z kilkoma snapshotami: `workmux snapshot prune --keep 2`, potem `snapshot list`. Sprawdź `snapshot prune` w audit logu | Sprawdzone ręcznie na tymczasowym repo (3 snapshoty, keep 1) |
| `guard.snapshot_keep` | Ustaw `guard: {snapshot_keep: 2}`, zrób 4× `workmux snapshot save`, potem `snapshot list`: mają zostać 2 | Sprawdzone tylko ręcznie na tymczasowym repo (patrz niżej), nie z prawdziwym agentem |
| fail-closed profilu | Zepsuj globalny config (np. `safety_profile: [oops`): `merge`/`remove`/`rebase` mają odmówić z podaniem błędu, a `list`/`status` działać. Napraw plik i sprawdź powrót do normy. Sprawdź, czy dashboard/sidebar nie ma własnej ścieżki merge omijającej profil | Sprawdzone ręcznie z `XDG_CONFIG_HOME` na `merge` i `snapshot list` |
| `workmux handoff` | W worktree z kilkoma commitami i niezacommitowanymi zmianami: `workmux handoff` i `--json`. Sprawdź, czy lista commitów i plików zgadza się z rzeczywistością oraz jak zachowuje się na branchu bez commitów | Sprawdzone tylko na tym repo |
| `workmux snapshot` | W prawdziwym worktree z agentem: `snapshot save`, pozwól agentowi coś zepsuć, `snapshot restore <id>`. Sprawdź pliki nieśledzone, ignorowane (`node_modules`), usunięte, oraz `snapshot restore` w audit logu | Sprawdzone tylko na małym tymczasowym repo; nie na dużym projekcie ani z agentem |
| nagłówek dashboardu: guard/collisions | Ustaw `guard.protected_paths` (np. `.env*`), w worktree zmień taki plik i otwórz dashboard: w nagłówku ma być `⚠ guard 1` w ciągu ~minuty. Zmień ten sam plik w drugim worktree: `collisions N`. Sprawdź, że przy zero wskaźnik znika i że dashboard nie zwalnia na dużym repo (liczenie co 60 s w tle) | Tylko test jednostkowy etykiety i budowanie; nie oglądane w prawdziwym dashboardzie |
