
# Working on the suite's code

The user may ask you to change tetron-wm or another part of the suite, then
run the result. This is the loop:

1. Code lives in `{{CODE_DIR}}`. Clone a part there when it is missing:
   `git clone https://github.com/tetronos/<part> {{CODE_DIR}}/<part>`.
2. Read the repository's own `AGENTS.md` first. Its rules win over this file
   inside that repository.
3. Spec first, where the repository has `spec/`: edit the spec, review it
   with `uv run libspec diff`, then implement; `python3 reconcile.py` must
   exit 0 before a commit.
4. Work on a branch, never on `main`. Keep the tests green (`cargo test`,
   `cargo clippy --all-targets` for the Rust parts) and add a test for the fix.
5. Run your build of tetron-wm in place of the installed one (an "own build"):

   ```sh
   TETRON_WM_METHOD=binary TETRON_WM_FROM_CLONE={{CODE_DIR}}/tetron-wm sh {{CODE_DIR}}/tetron-wm/install.sh
   tetron-wm reload
   ```

   Write the exact command in the chat before you run it, so the user can see
   it and run it by hand instead. It builds with `cargo build --release`
   first and changes nothing when the build fails. On a host with the .deb it
   diverts the packaged binary; `TETRON_WM_METHOD=deb sh install.sh` goes back.
   `tetron-wm reload` restarts the desktop with the user's apps kept open.
6. Never push to `main`, tag, bump a version or publish a release; those are
   the user's. Open a pull request only when the user asks for one.
