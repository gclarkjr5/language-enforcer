# Development Conventions & Checklist

This file guides AI assistants and developers on required steps when making code changes to the Language Enforcer project.

## Git Worktree Requirement

**ALL development work MUST be done in a git worktree, never in the main working directory.**

### Why Use Worktrees?
- Keeps main branch clean and always deployable
- Allows parallel development without stashing changes
- Prevents accidental commits to main
- Easy to discard failed experiments without cleanup

### Creating a Worktree

**For new features or changes:**
```bash
# Create a new branch and worktree in one command
git worktree add ../language-enforcer-feature-name -b feature/feature-name

# Navigate to the worktree
cd ../language-enforcer-feature-name
```

**For existing branches:**
```bash
# Check out existing branch in a worktree
git worktree add ../language-enforcer-bugfix bugfix/issue-123

cd ../language-enforcer-bugfix
```

### Worktree Naming Convention
```
../language-enforcer-<branch-type>-<description>
```

Examples:
- `../language-enforcer-feat-claude-api`
- `../language-enforcer-fix-translation-bug`
- `../language-enforcer-refactor-auth-server`
- `../language-enforcer-docs-api-migration`

### Working in a Worktree

1. **Create the worktree** (see above)
2. **Make your changes** in the worktree directory
3. **Run all tests and checks** (see Post-Change Requirements)
4. **Commit your work** following commit conventions
5. **Push the branch** when ready for review/merge
6. **Merge via PR** (or locally if appropriate)
7. **Clean up the worktree** (see below)

### Listing Active Worktrees
```bash
git worktree list
```

### Removing a Worktree

**After merging your changes:**
```bash
# From main repo directory
git worktree remove ../language-enforcer-feature-name

# Delete the branch if no longer needed
git branch -d feature/feature-name
```

**Force remove (if needed):**
```bash
git worktree remove --force ../language-enforcer-feature-name
```

### Worktree Best Practices

- **One worktree per feature/fix** - Keep changes isolated
- **Clean up merged worktrees** - Don't let them accumulate
- **Never commit in main directory** - Always use a worktree
- **Share .env between worktrees** - Symlink if needed:
  ```bash
  ln -s /path/to/main/.env .env
  ```

### AI Assistant Worktree Protocol

**When asked to make changes, AI assistants should:**

1. **Check current directory**
   ```bash
   pwd
   git rev-parse --abbrev-ref HEAD
   ```

2. **If in main directory, STOP and ask:**
   - "I notice we're in the main directory. Should I create a worktree for this work?"
   - "What should I name this feature branch?"

3. **Create worktree with user's approval**
   ```bash
   git worktree add ../language-enforcer-<name> -b <branch-name>
   cd ../language-enforcer-<name>
   ```

4. **Proceed with changes** only after in worktree

5. **After completion, remind user to:**
   - Review changes
   - Push branch
   - Create PR (if applicable)
   - Clean up worktree after merge

### Exception: Documentation-Only Changes

Small documentation updates (typos, clarifications) MAY be done directly on main if:
- Changes are trivial (< 10 lines)
- No code is modified
- User explicitly requests direct commit

## Pre-Change Checklist

### Before Starting Any Work
- [ ] **Verify you're in a git worktree** (not main directory)
- [ ] Branch name follows convention: `feat/`, `fix/`, `chore/`, `docs/`, `refactor/`
- [ ] Worktree directory is outside main repo (e.g., `../language-enforcer-<name>`)

### Before Modifying Rust Code
- [ ] Read relevant files first (never edit without reading)
- [ ] Understand the module structure (TUI, GUI, auth-server, core)
- [ ] Check if changes affect the shared `core` crate

### Before Modifying Frontend Code
- [ ] Verify you're in the correct directory (`gui/frontend/`)
- [ ] Check if Svelte components follow existing patterns
- [ ] Consider impact on Tauri bindings

## Post-Change Requirements

### After Any Rust Changes

**Always run these in order:**

1. **Format the code**
   ```bash
   cargo fmt --all
   ```

2. **Check for compilation errors**
   ```bash
   cargo check --workspace
   ```

3. **Run tests**
   ```bash
   cargo test --workspace
   ```

4. **Clippy for warnings** (recommended but optional)
   ```bash
   cargo clippy --workspace -- -W clippy::all
   ```

### After Auth Server Changes

**Required steps:**

1. **Build the auth-server**
   ```bash
   cargo build -p auth-server
   ```

2. **Verify environment variables are documented**
   - Check if new env vars need to be added to `.env.example`
   - Update README.md if API changes affect deployment

3. **Test all AI endpoints if modified**
   - `/ai/generate-sentence`
   - `/ai/generate-question`
   - `/ai/cleanup`
   - `/ai/grade-sentence`

### After TUI Changes

**Required steps:**

1. **Build and test the TUI**
   ```bash
   cargo build -p tui
   cargo run -p tui  # Manual smoke test
   ```

2. **Verify database interactions**
   - Ensure SQLite operations don't break existing `data/words.db`
   - Check that migrations are backward compatible

### After GUI/Frontend Changes

**Required steps:**

1. **Lint and build frontend**
   ```bash
   cd gui/frontend
   npm run build
   cd ../..
   ```

2. **Test Tauri integration** (if modifying Tauri commands)
   ```bash
   cd gui
   cargo tauri dev
   ```

3. **Verify mobile compatibility** (if UI changes)
   ```bash
   cargo tauri ios dev  # macOS only
   ```

### After Database Schema Changes

**Critical steps:**

1. **Update both SQLite and Postgres schemas**
   - Modify initialization in TUI/GUI
   - Update Neon schema documentation in README.md

2. **Create/update migration scripts**
   - Add migration to `scripts/` if needed
   - Test migration on existing databases

3. **Update affected queries in all workspaces**
   - Search for table usage: `rg "table_name" --type rust`

## Security & Best Practices

### Environment Variables
- **Never commit** `.env` files
- **Always update** `.env.example` when adding new variables
- **Document** all required environment variables in README.md

### API Keys
- Use environment variables only
- Never hardcode keys in source files
- Test that code gracefully handles missing keys

### Dependencies
- Pin critical dependencies in `Cargo.toml`
- Run `cargo update` carefully and test thoroughly
- Document breaking changes in commit messages

## Code Quality Standards

### Rust
- Follow Rust 2021 edition idioms
- Use `clippy` suggestions where reasonable
- Prefer explicit error handling over `.unwrap()`
- Add `// SAFETY:` comments for any `unsafe` blocks

### TypeScript/JavaScript
- Use consistent quotes (prefer single quotes)
- Avoid `any` types where possible
- Add JSDoc comments for complex functions

### Svelte
- Keep components under 300 lines
- Extract reusable logic to `lib/` directory
- Use stores for shared state

## Testing Philosophy

### What to Test
- Database operations (SQLite & Postgres compatibility)
- API endpoint responses (auth-server)
- Translation/cleanup workflows
- Card scheduling logic

### What Doesn't Need Tests
- Simple getters/setters
- UI component styling
- Environment variable loading

## Documentation Requirements

### When to Update Documentation

**Update README.md when:**
- Adding new commands or workflows
- Changing environment variables
- Modifying deployment process
- Adding new dependencies with setup requirements

**Update PROMPT.md when:**
- Changing core acceptance criteria
- Adding major features
- Modifying AI prompt behavior

**Update code comments when:**
- Adding complex algorithms
- Using non-obvious patterns
- Implementing workarounds

## Git Commit Conventions

### Commit Message Format
```
<type>: <short description>

<optional longer description>
```

**Types:**
- `feat:` - New feature
- `fix:` - Bug fix
- `chore:` - Maintenance (deps, formatting)
- `docs:` - Documentation only
- `refactor:` - Code restructuring
- `test:` - Adding/updating tests

### Examples
```
feat: add Claude API support to auth-server
fix: handle missing translation in cleanup workflow
chore: update dependencies and run cargo fmt
docs: clarify Neon schema setup in README
```

## AI-Specific Guidelines

### When Making Changes with AI Assistance

1. **Always read files before editing** - Never guess at file contents
2. **Run formatters immediately** - `cargo fmt` after Rust changes
3. **Test the specific feature** - Don't just compile, actually test functionality
4. **Verify environment setup** - Check required env vars are documented
5. **Update this file** - If new conventions emerge, add them here

### Common Pitfalls to Avoid

- Don't modify `target/` or `node_modules/` directories
- Don't change workspace structure without updating all `Cargo.toml` files
- Don't add new Tauri permissions without documenting them
- Don't change AI prompts without considering CEFR B1 level requirement
- Don't modify SQLite schema without parallel Postgres changes

## Quick Reference Commands

### Build Everything
```bash
cargo build --workspace --release
cd gui/frontend && npm run build && cd ../..
```

### Clean Build (when things break)
```bash
cargo clean
rm -rf gui/frontend/node_modules
cd gui/frontend && npm install && cd ../..
cargo build --workspace
```

### Run Full Test Suite
```bash
cargo fmt --all --check
cargo test --workspace
cd gui/frontend && npm run build && cd ../..
```

## Project-Specific Requirements

### B1 CEFR Level
All AI prompts **must** target CEFR B1 level (intermediate) for:
- Generated sentences
- Translations
- Questions
- Feedback

### Cross-Platform Compatibility
Changes must work on:
- macOS (required for iOS development)
- Linux (TUI primary target)
- iOS (via Tauri mobile)

### Offline-First Philosophy
- GUI must work without auth-server
- Local SQLite is source of truth
- Sync to Neon is opportunistic, not required
