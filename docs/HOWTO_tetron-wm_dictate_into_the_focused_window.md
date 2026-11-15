# How to dictate into the focused window

Dictation is an optional addon. tetron-wm contains no speech code, no model and no audio library. It starts [tetron-stt](https://github.com/tetronos/tetron-stt) (`tetron-stt serve`) as a provider, shows what it hears, and types the finished text into the focused window, so it works with any program in a tetron-wm window: a shell, an editor, claude, opencode, pi. Without tetron-stt installed the leader keys only show a message saying it is needed.

## What you need

tetron-stt installed (from the Store), and its speech models, about 0.9 GB, downloaded once with **Settings → Dictation → Download models** (or `tetron-stt models download`). Models are never downloaded automatically. A microphone on the computer where the tetron-wm client runs: the client starts the provider, so a local session uses the local microphone.

## Use

| Action | How |
| --- | --- |
| Start or stop listening | leader then `d`, or click `MIC` in the menubar |
| Switch mode | leader then `D` |

The menubar shows `MIC` when idle, `MIC ...` while the models load (the first start takes a few seconds), and `MIC ON live` (or `MIC ON final`) while listening. The live text appears on the bottom line of the screen as you speak. When you pause, the text is committed: it is typed into the focused window, the same way as a paste, so it can not execute anything by itself. If the sentence ends with the enter word (default `make it so`), Enter is pressed after the text.

Modes: `segment` (default) shows live text and commits it after each pause. `final` listens until you stop, then does one more accurate pass over the whole recording.

## Settings

In `config.toml`, `--help`, or **Settings → Dictation**. `dictation_command`: the provider program, empty uses `tetron-stt`. `dictation_mode`: `segment` or `final`. `dictation_pause`: seconds of silence that end a segment (1.2). `dictation_enter_word`: the spoken phrase that presses Enter, empty disables it. `dictation_threads`: CPU threads, 0 lets the provider choose. Changes apply the next time the provider starts: detach and attach, or `tetron-wm reload`. The keys are the `dictate` and `dictate_mode` actions in `[keys]`.

## Over ssh

A window running `ssh host` inside a local tetron-wm works: the text is typed into the ssh window and goes through to the remote side. A remote tetron-wm itself (after switching system, or `ssh host` then `tetron-wm`) has no access to the microphone of the computer you sit at, and is not supported yet.

## The provider protocol

For other speech engines: the program is started as `PROGRAM serve [--mode M] [--pause S] [--enter-word W] [--threads N]` and talks JSON lines. tetron-wm sends `{"cmd":"toggle"}`, `{"cmd":"mode"}`, `{"cmd":"mode","mode":"final"}`, `{"cmd":"start"}`, `{"cmd":"stop"}` and `{"cmd":"quit"}`; EOF on stdin quits it. The provider sends `{"state":"idle|loading|listening|finishing","mode":"segment"}`, `{"partial":"text so far"}`, `{"commit":"text","enter":false}`, `{"error":"..."}` and `{"info":"..."}`. Unknown messages and fields are ignored.

## Troubleshooting

`dictation needs tetron-stt`: install it from the Store. An error on the bottom line (no microphone, models missing): it comes from the provider, run `tetron-stt doctor` in a terminal. Nothing is typed: the window must be focused, and the app must accept pasted text. See also tetron-stt's own README.

## See also

`docs/KEYS_tetron-wm_leader_key_table.md` (`d` and `D`), `docs/CONFIG.md` (the `dictation_*` keys).
