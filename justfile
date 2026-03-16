install:
    cargo install --path crates/pensa
    cargo install --path crates/ralph
    cargo install --path crates/springfield
    cargo install --path crates/claude-wrapper
    cargo install --path crates/forma
    rsync -av --delete --exclude='logs/' --exclude='run/' --exclude='.DS_Store/' .sgf/ ~/.sgf/
