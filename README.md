# middy: a fast and full-featured MIDI library for Python and Rust

(NB: all this text was written by a human person.)

## WARNING: rapid change happening

This project is feature-complete, in use and seems to be solid, but the API will be
under continuous change for the next couple of weeks until the release, all in the
direction of simplicity.

## What is it?

`middy` is a full-featured MIDI library available in Rust and Python, for Linux, MacOS
and Windows.

Using the popular low-level Rust MIDI library
[`midir`](https://github.com/Boddlnagg/midir) as a transport layer, it adds not just
Python bindings, but full support for MIDI 2, 7- and 14- bit controllers, sysex, MTS
(MIDI Tuning Standard), MPE (MIDI polyphonic expression), and Python bindings.

On the Python side only, all parameters that accept numbers with implied units, like
times, frequencies, rates and that sort of thing, can accept strings with units (like
'500 ms' or '120 bpm') using ['pint'](https://github.com/hgrecco/pint).

Python is not a bolt-on. This project has been a Python/Rust combination since the
start, and every effort has been expended to make the API as Pythonic as possible, where
complex and complicated things should be possible, but simple things should be easy.

The [Python](doc/examples.md) and [Rust](doc/rust-examples.md) examples say more.
(NB: these documents are also changing fast as of 2026/11/12.)

## Why?

I've been writing digital audio and music programs since the late 1970s.

These days I write with Python and Rust (and a little C++).

In Python, I used the `mido` library, but it doesn't support a lot of newer features and
as I did more Rust development, I wanted a MIDI library that worked much the same in
Rust and in Python.

Now we're coding assistant enhanced, so I spent a day listing the very many features I
needed and how they went, and got it to write a Python and Rust library.

Then I spent a couple of weeks slowly simplifying and decruftifying the API and data
structures, hiding most of the machinery for most of the users.

In a week or two it will be done and then I can inflict it on an unsuspecting world.
