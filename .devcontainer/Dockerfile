FROM node:22-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
      git ripgrep curl ca-certificates openssh-client \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g @anthropic-ai/claude-code

# non-root user (the node image ships with uid 1000 "node")
USER node
WORKDIR /workspace
ENV DISABLE_AUTOUPDATER=1
CMD ["claude"]
