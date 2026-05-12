alias i := install

install:
    cargo install --path crates/pensa

    cargo install --path crates/springfield
    cargo install --path crates/claude-wrapper
    cargo install --path crates/forma
    mkdir -p ~/.sgf/logs ~/.sgf/run
    ln -sfn "$(pwd)/.sgf/MEMENTO.md" ~/.sgf/MEMENTO.md
    ln -sfn "$(pwd)/.sgf/BACKPRESSURE.md" ~/.sgf/BACKPRESSURE.md
    # ln -sfn can't replace a real directory — remove stale rsync copies first
    [ -d ~/.sgf/cursus ] && [ ! -L ~/.sgf/cursus ] && rm -rf ~/.sgf/cursus || true
    [ -d ~/.sgf/prompts ] && [ ! -L ~/.sgf/prompts ] && rm -rf ~/.sgf/prompts || true
    ln -sfn "$(pwd)/.sgf/cursus" ~/.sgf/cursus
    ln -sfn "$(pwd)/.sgf/prompts" ~/.sgf/prompts
