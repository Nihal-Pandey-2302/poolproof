# Install

Binaries for Linux and macOS (x86_64 and arm64) and Windows (x86_64) are on the
[releases page](https://github.com/Nihal-Pandey-2302/poolproof/releases), with SHA256SUMS
and a build attestation (`gh attestation verify FILE --repo Nihal-Pandey-2302/poolproof`).
To build it yourself (it isn't on crates.io yet):

```sh
cargo install --locked --git https://github.com/Nihal-Pandey-2302/poolproof --tag v0.1.0 poolproof
```

Leave out `--tag v0.1.0` for the code on main. The last `poolproof` picks the package,
since the repository holds other, internal ones, and `--locked` builds with the
dependency versions in Cargo.lock, the ones CI tests. Building needs Rust 1.88 or newer,
a C compiler and libclang, since poolproof uses the Postgres parser (C, through bindgen):
`apt install clang libclang-dev` on Debian and Ubuntu, `dnf install clang clang-devel` on
Fedora, the Xcode command line tools (`xcode-select --install`) on macOS, and LLVM
(`winget install LLVM.LLVM`) with the Visual Studio C++ build tools on Windows.

The scenarios and the latest results are built into the binary.

The release archives also hold shell completions (`completions/`) and man pages (`man/`).
Installed another way, poolproof prints them itself:

```sh
poolproof completions bash > ~/.local/share/bash-completion/completions/poolproof
poolproof completions zsh > ~/.zfunc/_poolproof   # with fpath+=(~/.zfunc) in ~/.zshrc
poolproof completions fish > ~/.config/fish/completions/poolproof.fish
poolproof man > ~/.local/share/man/man1/poolproof.1   # `poolproof man run`: poolproof-run(1)
```

`completions` also takes `powershell` and `elvish`.
