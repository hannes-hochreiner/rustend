export def test [] {
  cargo test
  cargo check -p rustend-client --target wasm32-unknown-unknown
}

export def outdated [] {
  cargo outdated --workspace
}
