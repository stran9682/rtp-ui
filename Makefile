macos:
	@cargo build --release --lib --target aarch64-apple-darwin
	@cargo build --release --lib --target x86_64-apple-darwin
	@$(RM) -rf libs/rtp-macos.a
	@lipo -create -output libs/rtp-macos.a \
			target/aarch64-apple-darwin/release/librtp.a \
			target/x86_64-apple-darwin/release/librtp.a