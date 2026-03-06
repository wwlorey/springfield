FROM docker/sandbox-templates:claude-code

USER root

# System dependencies for Tauri development and general builds
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libwebkit2gtk-4.1-dev \
    webkit2gtk-driver \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libssl-dev \
    curl \
    wget \
    file \
    libxdo-dev \
    # Playwright browser dependencies (--with-deps fails on this Ubuntu version)
    libnss3 \
    libnspr4 \
    libatk1.0-0t64 \
    libatk-bridge2.0-0t64 \
    libcups2t64 \
    libdrm2 \
    libxcomposite1 \
    libxdamage1 \
    libxfixes3 \
    libxrandr2 \
    libgbm1 \
    libpango-1.0-0 \
    libcairo2 \
    libasound2t64 \
    libxshmfence1 \
    libglib2.0-0t64 \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
USER agent
ENV RUSTUP_HOME=/home/agent/.rustup
ENV CARGO_HOME=/home/agent/.cargo
ENV PATH="/home/agent/.cargo/bin:${PATH}"

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && . "$CARGO_HOME/env" \
    && rustup default stable \
    && rustup component add rustfmt clippy

# Install Tauri CLI, cargo-geiger (unsafe code auditing), and prek (git hook manager)
RUN . "$CARGO_HOME/env" && cargo install tauri-cli cargo-geiger prek

# Enable pnpm and yarn
USER root
RUN corepack enable && corepack prepare pnpm@latest --activate && corepack prepare yarn@stable --activate

# Install global JS tools
USER agent
ENV PNPM_HOME="/home/agent/.local/share/pnpm"
ENV PATH="${PNPM_HOME}:${PATH}"
ENV SHELL="/bin/bash"

RUN pnpm setup && \
    pnpm add -g \
    typescript \
    @tauri-apps/cli \
    playwright \
    madge \
    knip \
    type-coverage

# Install Playwright browsers
USER root
RUN npm install -g playwright && npx playwright install

# Install gitleaks (secrets scanner)
RUN GITLEAKS_VERSION=$(curl -s https://api.github.com/repos/gitleaks/gitleaks/releases/latest | grep '"tag_name"' | sed 's/.*"v\(.*\)".*/\1/') && \
    curl -sSfL "https://github.com/gitleaks/gitleaks/releases/download/v${GITLEAKS_VERSION}/gitleaks_${GITLEAKS_VERSION}_linux_amd64.tar.gz" | tar xz -C /usr/local/bin gitleaks

USER agent

# Install pensa CLI from source
USER agent
COPY --chown=agent:agent pensa-src /tmp/pensa-src
RUN . "$CARGO_HOME/env" && cargo install --path /tmp/pensa-src && rm -rf /tmp/pensa-src

ENV CARGO_TARGET_DIR="/home/agent/target"

# Ensure agent owns their home directory
RUN chown -R agent:agent /home/agent

USER agent
WORKDIR /home/agent

# Verify installations
RUN rustc --version && cargo --version && cargo geiger --version && prek --version && node --version && pnpm --version && yarn --version && npx playwright --version && gitleaks version && madge --version && knip --version && pn --help
