# Springfield Implementation Plan

Gaps between `specs/springfield.md` and `crates/springfield/` — organized by area of concern, with spec citations and source code references.

---

## 1. Scaffold Filenames: Uppercase Convention

**Spec** (springfield.md §sgf init — What it creates): File tree shows `.sgf/BACKPRESSURE.md`, `.sgf/PENSA.md`, `MEMENTO.md` (uppercase).

**Code**: `crates/springfield/src/init.rs` uses lowercase paths throughout:
- Line 58–60: `.sgf/backpressure.md`
- Line 89–91: `.sgf/pensa.md`
- Line 108: `memento.md`

**Changes needed**:
- `crates/springfield/src/init.rs` — Rename all three paths to uppercase in `TEMPLATE_FILES` and `SKELETON_FILES` constants
- `crates/springfield/templates/*.md` — Update any internal references to these filenames (e.g., build.md line 13 references `.sgf/backpressure.md`, should be `.sgf/BACKPRESSURE.md`; build.md line 3 references `.sgf/pensa.md`, should be `.sgf/PENSA.md`)
- `crates/springfield/src/init.rs` — Update `MEMENTO_CONTENT` references (covered by item 2 below)
- `crates/springfield/src/prompt.rs` — Update MEMENTO.md path reference when adding memento prepend (item 5)
- Unit tests in `init.rs` (lines 394–708) — Update file path assertions
- Integration tests in `tests/integration.rs` (lines 126–145, 154–194) — Update path and content assertions

---

## 2. MEMENTO.md Content

**Spec** (springfield.md §sgf init — MEMENTO.md):
```markdown
study `specs/README.md`
study `.sgf/BACKPRESSURE.md`
study `.sgf/PENSA.md`
```

**Code**: `crates/springfield/src/init.rs` lines 18–30 define `MEMENTO_CONTENT` as a structured markdown document with `# Memento`, `## Stack`, `## References` sections — completely different format.

**Changes needed**:
- `crates/springfield/src/init.rs` — Replace `MEMENTO_CONTENT` constant with the three `study` directives from the spec
- `crates/springfield/src/init.rs` — Update `memento_content` test (lines 445–452) to assert for `study` directives instead of `## Stack` / `## References`
- `crates/springfield/tests/integration.rs` — Update `init_file_contents` test (lines 159–164) to check for `study` directives

---

## 3. CLAUDE.md as Symlink

**Spec** (springfield.md §sgf init — CLAUDE.md): "`ln -s` to AGENTS.md"

**Spec** (springfield.md §Per-Repo Project Structure — File Purposes): "**`CLAUDE.md`** — Entry point for Claude Code. Symlinks to AGENTS.md."

**Code**: `crates/springfield/src/init.rs` lines 32–33 define `CLAUDE_MD_CONTENT` and lines 109–112 write it as a regular file via `SKELETON_FILES`.

**Changes needed**:
- `crates/springfield/src/init.rs` — Remove `CLAUDE.md` from `SKELETON_FILES` and the `CLAUDE_MD_CONTENT` constant
- `crates/springfield/src/init.rs` — Add symlink creation in `run()`: `std::os::unix::fs::symlink("AGENTS.md", root.join("CLAUDE.md"))`, skipping if file/symlink already exists (idempotency)
- `crates/springfield/src/init.rs` — Update `claude_md_content` test (lines 436–442) to verify it's a symlink pointing to `AGENTS.md`
- `crates/springfield/src/init.rs` — Update `does_not_overwrite_existing_files` test (lines 455–473) for symlink
- `crates/springfield/tests/integration.rs` — Update `init_file_contents` test (lines 153–155) and `init_idempotent` test (lines 197–233)

---

## 4. specs/README.md Heading

**Spec** (springfield.md §sgf init — specs/README.md):
```markdown
# Specifications

| Spec | Code | Purpose |
|------|------|---------|
```

**Code**: `crates/springfield/src/init.rs` lines 34–39 define `SPECS_README_CONTENT` with heading `# Specs` instead of `# Specifications`.

**Changes needed**:
- `crates/springfield/src/init.rs` — Change `SPECS_README_CONTENT` heading from `# Specs` to `# Specifications`

---

## 5. Prompt Assembly: MEMENTO.md Prepend

**Spec** (springfield.md §Prompt Assembly — Assembly Process):
> 1. Read `MEMENTO.md` from the project root
> 2. Read the template from `.sgf/prompts/<stage>.md`
> 3. Substitute variables
> 4. Validate — scan for unresolved `{{...}}` tokens
> 5. **Prepend the memento content before the template content**
> 6. Write the assembled prompt to `.sgf/prompts/.assembled/<stage>.md`
> 7. Pass the file path as ralph's `PROMPT` argument

**Code**: `crates/springfield/src/prompt.rs` `assemble()` function (lines 6–36) reads the template, substitutes variables, validates, and writes — but never reads or prepends `MEMENTO.md`.

**Changes needed**:
- `crates/springfield/src/prompt.rs` — In `assemble()`, before variable substitution:
  1. Read `root.join("MEMENTO.md")` content (fail gracefully if missing — warn but continue, since MEMENTO.md might not exist in non-init'd projects)
  2. After substitution and validation, prepend memento content with a newline separator before the template content
- `crates/springfield/src/prompt.rs` — Add unit tests:
  - Test that MEMENTO.md content is prepended to assembled output
  - Test that assembly still works when MEMENTO.md is absent (graceful fallback)
  - Test that unresolved token validation applies only to template content, not memento content
- `crates/springfield/tests/integration.rs` — Add integration test verifying MEMENTO.md appears at the top of assembled prompts

---

## 6. Prompt Templates: Remove Redundant `Read memento.md`

**Spec** (springfield.md §Prompt Templates): None of the spec template contents include `Read memento.md` because sgf handles memento injection automatically (§Prompt Assembly step 5).

**Code**: Every template file in `crates/springfield/templates/` starts with `Read \`memento.md\`.`:
- `templates/spec.md` line 1
- `templates/build.md` line 1
- `templates/verify.md` line 1
- `templates/test-plan.md` line 1
- `templates/test.md` line 1
- `templates/issues.md` line 1
- `templates/issues-plan.md` line 1

**Changes needed**:
- `crates/springfield/templates/spec.md` — Remove `Read \`memento.md\`.` line and blank line after it
- `crates/springfield/templates/build.md` — Remove `Read \`memento.md\`.` line and blank line after it
- `crates/springfield/templates/verify.md` — Remove `Read \`memento.md\`.` line and blank line after it
- `crates/springfield/templates/test-plan.md` — Remove `Read \`memento.md\`.` line and blank line after it
- `crates/springfield/templates/test.md` — Remove `Read \`memento.md\`.` line and blank line after it
- `crates/springfield/templates/issues.md` — Remove `Read \`memento.md\`.` line and blank line after it
- `crates/springfield/templates/issues-plan.md` — Remove `Read \`memento.md\`.` line and blank line after it

Additionally, verify.md and test-plan.md have `Read \`specs/README.md\`.` as a second line — these stay because they are part of the spec's template content.

---

## 7. Prompt Templates: Uppercase References

**Spec** (springfield.md §Prompt Templates — build.md): References `.sgf/PENSA.md` and `.sgf/BACKPRESSURE.md` (uppercase).

**Code**: Templates reference lowercase filenames:
- `templates/build.md` line 3: `.sgf/pensa.md` → should be `.sgf/PENSA.md`
- `templates/build.md` line 13: `.sgf/backpressure.md` → should be `.sgf/BACKPRESSURE.md`
- `templates/test.md` line 9: `.sgf/pensa.md` → should be `.sgf/PENSA.md`
- `templates/test.md` line 17: `.sgf/backpressure.md` → should be `.sgf/BACKPRESSURE.md`
- `templates/issues-plan.md` line 3: `.sgf/pensa.md` → should be `.sgf/PENSA.md`

**Changes needed**:
- `crates/springfield/templates/build.md` — Update `.sgf/pensa.md` → `.sgf/PENSA.md`, `.sgf/backpressure.md` → `.sgf/BACKPRESSURE.md`
- `crates/springfield/templates/test.md` — Update `.sgf/pensa.md` → `.sgf/PENSA.md`, `.sgf/backpressure.md` → `.sgf/BACKPRESSURE.md`
- `crates/springfield/templates/issues-plan.md` — Update `.sgf/pensa.md` → `.sgf/PENSA.md`

---

## 8. Backpressure Template Content

**Spec** (springfield.md §Backpressure Template): Defines exact template content with:
- Bold intro: `**After making changes, apply FULL BACKPRESSURE to verify behavior as appropriate.**`
- Single `## Frontend` section with subsections for Tauri, Playwright E2E, and Tauri E2E
- Uses `pnpm run build`, `pnpm run vitest run`, etc. (with `run` subcommand)
- Includes `Full check: pnpm run check`
- No "Model-Dependent Tests" section

**Code**: `crates/springfield/templates/backpressure.md` has:
- Non-bold intro (line 3): `After making changes, apply FULL BACKPRESSURE to verify behavior.`
- "Model-Dependent Tests" section (lines 24–33) — not in spec
- Two separate frontend sections: "Frontend (Tauri, SvelteKit)" (line 36) and "Frontend (SvelteKit, Vite)" (line 77) — spec has one unified section
- Uses `pnpm build`, `pnpm vitest run` (without `run`) — spec uses `pnpm run build`, `pnpm run vitest run`
- Project-specific details like `BUDDY_MOCK_AUDIO`, `wdio.conf.js` — not in spec

**Changes needed**:
- `crates/springfield/templates/backpressure.md` — Replace entire content with the spec's backpressure template verbatim (springfield.md §Backpressure Template)

---

## 9. Dockerfile: Add Playwright

**Spec** (springfield.md §Docker Sandbox Template — Dockerfile): Includes Playwright installation:
```dockerfile
# Install Playwright browsers
RUN pnpm exec playwright install --with-deps
```
And the verify line includes `pnpm exec playwright --version`:
```dockerfile
RUN rustc --version && cargo --version && node --version && pnpm --version && pnpm exec playwright --version && pn --help
```

**Code**: `.docker/sandbox-templates/ralph/Dockerfile` is missing the Playwright install section entirely (between lines 47 and 49). The verify line (line 61) does not include `pnpm exec playwright --version`.

**Changes needed**:
- `.docker/sandbox-templates/ralph/Dockerfile` — Add `RUN pnpm exec playwright install --with-deps` between the pnpm global tools install and the pensa CLI install
- `.docker/sandbox-templates/ralph/Dockerfile` — Add `pnpm exec playwright --version` to the verify line
- `crates/springfield/src/template.rs` — Update tests that assert on Dockerfile content if needed (current tests at lines 223–244 don't check for Playwright, but adding a test would catch future regressions)

---

## 10. Gitignore: CLAUDE.md Entry

**Spec** (springfield.md §sgf init — Gitignore): The gitignore entries listed do not include `CLAUDE.md`.

**Code**: `crates/springfield/src/init.rs` — The gitignore entries (lines 119–160) match the spec.

**Status**: No changes needed. Matches spec.

---

## 11. init.rs: GITIGNORE_ENTRIES Constant Alignment

**Spec** (springfield.md §sgf init — Entries added): Lists exact gitignore entries to add.

**Code**: `crates/springfield/src/init.rs` lines 119–160 — Current entries match the spec entries.

**Status**: No changes needed. Matches spec.

---

## 12. Pre-commit Config

**Spec** (springfield.md §sgf init — Prek hooks): Specifies exact YAML content.

**Code**: `crates/springfield/src/init.rs` lines 169–185 — Matches spec.

**Status**: No changes needed. Matches spec.

---

## 13. Claude Deny Settings

**Spec** (springfield.md §sgf init — Claude deny settings): Specifies exact deny rules.

**Code**: `crates/springfield/src/init.rs` lines 162–167 — Matches spec.

**Status**: No changes needed. Matches spec.

---

## Documentation

### README.md Updates

- `README.md` (repo root, lines 49–56) — Update the "Scaffold a Project" section to reflect uppercase filenames (`MEMENTO.md`, `CLAUDE.md`). Currently says: "creates `.sgf/`, `.pensa/`, `specs/`, prompt templates, `MEMENTO.md`, `CLAUDE.md`" — the naming is correct in the README but verify it matches post-implementation filenames.
- `README.md` — Verify the "Usage" section (lines 62–71) still accurately reflects the CLI commands after any changes.

### AGENTS.md Updates

- `AGENTS.md` (repo root) — No changes needed. This file is hand-authored and not generated by `sgf init`.

### Spec/Code Cross-References

- `specs/springfield.md` — No changes needed. The spec is the source of truth; the code is being brought into alignment.
- `specs/readme.md` — No changes needed. Already correctly references `crates/springfield/`.

---

## Integration Tests

The following integration tests should be added to `crates/springfield/tests/integration.rs` to verify full end-to-end correctness. All tests are designed to run from the command line via `cargo test -p springfield`.

### Test 1: `init_uppercase_filenames`

Verify `sgf init` creates files with uppercase names.

```
Setup: setup_test_dir()
Run: sgf init
Assert:
  - .sgf/BACKPRESSURE.md exists (not .sgf/backpressure.md)
  - .sgf/PENSA.md exists (not .sgf/pensa.md)
  - MEMENTO.md exists (not memento.md)
  - .sgf/loom-specs-README.md exists (lowercase — correct per spec)
```

### Test 2: `init_memento_content`

Verify `MEMENTO.md` contains `study` directives per spec.

```
Setup: setup_test_dir()
Run: sgf init
Assert:
  - MEMENTO.md contains "study `specs/README.md`"
  - MEMENTO.md contains "study `.sgf/BACKPRESSURE.md`"
  - MEMENTO.md contains "study `.sgf/PENSA.md`"
  - MEMENTO.md does NOT contain "## Stack" or "## References"
```

### Test 3: `init_claude_md_is_symlink`

Verify `CLAUDE.md` is created as a symlink to `AGENTS.md`.

```
Setup: setup_test_dir()
Run: sgf init
Assert:
  - CLAUDE.md exists
  - CLAUDE.md is a symlink (fs::symlink_metadata → is_symlink())
  - Symlink target is "AGENTS.md" (fs::read_link)
```

### Test 4: `init_specs_readme_heading`

Verify `specs/README.md` has the correct heading.

```
Setup: setup_test_dir()
Run: sgf init
Assert:
  - specs/README.md starts with "# Specifications"
  - specs/README.md does NOT start with "# Specs"
```

### Test 5: `prompt_assembly_prepends_memento`

Verify assembled prompts include `MEMENTO.md` content at the top.

```
Setup:
  - setup_test_dir()
  - sgf init
  - git add . && git commit
  - Write MEMENTO.md with "study `specs/README.md`\nstudy `.sgf/BACKPRESSURE.md`\nstudy `.sgf/PENSA.md`"
Run: springfield::prompt::assemble(root, "verify", &HashMap::new())
Assert:
  - Assembled content starts with MEMENTO.md content
  - MEMENTO.md content appears BEFORE the verify template content
  - Template content is still present after the memento
```

### Test 6: `prompt_assembly_without_memento`

Verify prompt assembly still works when `MEMENTO.md` is absent.

```
Setup:
  - Create .sgf/prompts/.assembled/ and .sgf/prompts/verify.md
  - Do NOT create MEMENTO.md
Run: springfield::prompt::assemble(root, "verify", &HashMap::new())
Assert:
  - Assembly succeeds (no error)
  - Assembled content equals the raw template content
```

### Test 7: `templates_no_read_memento_directive`

Verify none of the scaffolded templates contain `Read \`memento.md\`` since sgf handles injection.

```
Setup: setup_test_dir()
Run: sgf init
Assert:
  - .sgf/prompts/spec.md does NOT contain "Read `memento.md`"
  - .sgf/prompts/build.md does NOT contain "Read `memento.md`"
  - .sgf/prompts/verify.md does NOT contain "Read `memento.md`"
  - .sgf/prompts/test-plan.md does NOT contain "Read `memento.md`"
  - .sgf/prompts/test.md does NOT contain "Read `memento.md`"
  - .sgf/prompts/issues.md does NOT contain "Read `memento.md`"
  - .sgf/prompts/issues-plan.md does NOT contain "Read `memento.md`"
```

### Test 8: `templates_reference_uppercase_filenames`

Verify scaffolded templates reference `.sgf/PENSA.md` and `.sgf/BACKPRESSURE.md` (uppercase).

```
Setup: setup_test_dir()
Run: sgf init
Assert:
  - .sgf/prompts/build.md contains ".sgf/PENSA.md" (not ".sgf/pensa.md")
  - .sgf/prompts/build.md contains ".sgf/BACKPRESSURE.md" (not ".sgf/backpressure.md")
  - .sgf/prompts/test.md contains ".sgf/PENSA.md"
  - .sgf/prompts/test.md contains ".sgf/BACKPRESSURE.md"
  - .sgf/prompts/issues-plan.md contains ".sgf/PENSA.md"
```

### Test 9: `init_idempotent_with_uppercase`

Verify `sgf init` is idempotent with the new uppercase filenames.

```
Setup: setup_test_dir()
Run: sgf init (twice)
Assert:
  - All uppercase files still exist after second run
  - Content unchanged on second run
  - No duplicate gitignore/deny/hook entries
```

### Test 10: `end_to_end_build_loop_with_memento_injection`

Full end-to-end test: `sgf init` → modify MEMENTO.md → `sgf build auth -a` with mock ralph → verify assembled prompt contains memento content.

```
Setup:
  - setup_test_dir()
  - sgf init
  - git add . && git commit
  - Mock ralph that captures the assembled prompt content
  - Mock pn that exits 0
Run: sgf build auth -a (with SGF_RALPH_BINARY=mock, PATH including mock pn)
Assert:
  - Ralph was invoked
  - Assembled prompt at .sgf/prompts/.assembled/build.md:
    * Starts with MEMENTO.md content (study directives)
    * Contains the build template content (with {{spec}} replaced by "auth")
    * Does NOT contain "Read `memento.md`"
    * References ".sgf/PENSA.md" (uppercase)
    * References ".sgf/BACKPRESSURE.md" (uppercase)
```

### Tools Required

All tests use existing infrastructure:
- `tempfile::TempDir` for isolation
- `std::process::Command` for running `sgf` binary
- Mock shell scripts for ralph and pn
- `SGF_RALPH_BINARY` env var for ralph override
- `PATH` manipulation for mock pn

No additional tools are required.
