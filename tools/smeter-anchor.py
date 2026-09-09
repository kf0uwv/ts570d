#!/usr/bin/env python3
"""Find the one number the S-meter table cannot be derived without.

# What this is for

`SUnitScale::TS570D`'s thresholds decide which S-unit a raw `SM;` reading
is drawn as. Its *spacing* was measured on 2026-09-08 and is right: three
controlled preamp steps on three bands (8.0-8.8 dB each, the step measured
on the IF tap rather than assumed) moved the meter 2-3.5 raw counts, so a
raw count is about 2.8 dB and a 6 dB S-unit is about 2.1 counts. The table
uses 2 counts per S-unit, which is that, within the error of the method.

What no amount of software can establish is the **anchor**: which raw
value the radio itself calls S9. The CAT protocol will not say -- `SM;`
reports a bare number, and the manual documents it only as "S-METER
VALUE". The front panel knows, and the front panel cannot be read over a
serial cable.

So this asks you to read it. Twice, at two clearly different signal
levels, which is enough to place the anchor and to check the spacing
against your own meter rather than against mine.

# Running it

    python3 tools/smeter-anchor.py            # server on 127.0.0.1:4535

Park the radio on any steady signal you can see move the meter -- a
broadcast carrier in AM is ideal, an SSB station is not. The script never
transmits, never touches PC, and puts every setting back where it found
it, including if you interrupt it.

# The other thing this settles

The CAT reference gives `SM` a range of `0000~0015`; the code assumes
`0~30` and places S9 at raw 20. If the manual is right the radio can
never produce a reading the console calls S9. A sweep of eight broadcast
bands could not push the meter past 7, so it is unsettled -- see
troubleshooting-plan.md item 48.

**So the strongest signal you can find is the useful one here.** If the
raw value climbs above 15 the code is right. If it pins at 15 while the
panel meter keeps climbing past S9, the range and the S-unit table both
want halving. This script prints the highest raw it sees for exactly that
reason.
"""

import json
import socket
import statistics
import struct
import sys
import time

PORT = 4535
# An S-unit, by the convention every meter is nominally built to.
DB_PER_S_UNIT = 6.0


class Cat:
    def __init__(self, host="127.0.0.1", port=PORT, timeout=8):
        self.s = socket.create_connection((host, port), timeout=timeout)

    def __call__(self, frame):
        p = frame.encode()
        self.s.sendall(struct.pack(">I", len(p)) + p)
        hdr = b""
        while len(hdr) < 4:
            c = self.s.recv(4 - len(hdr))
            if not c:
                raise ConnectionError("server closed the connection")
            hdr += c
        n = struct.unpack(">I", hdr)[0]
        body = b""
        while len(body) < n:
            c = self.s.recv(n - len(body))
            if not c:
                break
            body += c
        return body.decode(errors="replace")

    def set(self, frame):
        # This port frames a reply even to a silent Kenwood set. Not
        # reading it makes every later query return the previous answer.
        self(frame)
        time.sleep(0.25)


def smeter(cat, n=20, gap=0.2):
    v = []
    for _ in range(n):
        r = cat("SM;")
        if r.startswith("SM") and r.endswith(";"):
            try:
                v.append(int(r[2:-1]))
            except ValueError:
                pass
        time.sleep(gap)
    return v


def ask_s_unit(prompt):
    """Read an S-unit off the operator, in the form the panel shows it."""
    while True:
        raw = input(prompt).strip().upper().replace(" ", "")
        if raw in ("Q", "QUIT", ""):
            return None
        if raw.startswith("S"):
            raw = raw[1:]
        # "9+20" and "9" are both things a panel shows.
        if "+" in raw:
            base, over = raw.split("+", 1)
        else:
            base, over = raw, "0"
        try:
            return float(base) * DB_PER_S_UNIT + float(over)
        except ValueError:
            print("  I did not understand that. Try: 5, or 9, or 9+20")


def main():
    cat = Cat()
    before = {q: cat(q) for q in ("FA;", "MD;", "PA;", "RA;", "AN;")}
    print("Radio state recorded; it goes back exactly here at the end.\n")
    try:
        print("Park the radio on a steady signal -- a broadcast carrier in")
        print("AM reads best. Do not use SSB speech; it never sits still.\n")
        input("Press Enter when the meter is settled. ")

        points = []
        for label, pre, att in (
            ("preamp ON, attenuator off", "1", "00"),
            ("preamp off, attenuator ON", "0", "01"),
        ):
            cat.set(f"PA{pre};")
            cat.set(f"RA{att};")
            time.sleep(1.5)
            v = smeter(cat)
            if not v:
                print("  the radio did not answer SM; -- is it on?")
                return 1
            raw = statistics.median(v)
            print(f"\n  {label}")
            print(f"  CAT reports raw {raw:.1f}  (min {min(v)}, max {max(v)})")
            if max(v) > 15:
                print("  ** raw above 15: the 0-30 range is right, the manual is not")
            db = ask_s_unit("  What does the FRONT PANEL read? (e.g. 5, 9, 9+20, or q) ")
            if db is None:
                print("\n  stopped at your request")
                return 1
            points.append((raw, db))

        (r1, d1), (r2, d2) = points
        if r1 == r2:
            print("\nBoth readings landed on the same raw value, so there is")
            print("no spread to measure. Try a stronger signal.")
            return 1

        db_per_count = (d1 - d2) / (r1 - r2)
        counts_per_s = DB_PER_S_UNIT / db_per_count if db_per_count else float("nan")
        # Raw at S9, from the line through the two points.
        s9_db = 9 * DB_PER_S_UNIT
        raw_at_s9 = r1 + (s9_db - d1) / db_per_count

        print("\n" + "=" * 62)
        print(f"  {db_per_count:5.2f} dB per raw count   "
              f"({counts_per_s:4.2f} raw counts per S-unit)")
        print(f"  S9 sits at raw {raw_at_s9:.1f}")
        print("=" * 62)
        print("\nSuggested SUnitScale::TS570D thresholds (inclusive upper")
        print("bound per label, S1..S9 then the over-S9 marks):\n")
        table = []
        for s in range(1, 10):
            table.append(round(raw_at_s9 - (9 - s) * counts_per_s))
        for over in (10, 20, 30):
            table.append(round(raw_at_s9 + over / db_per_count))
        table = [max(0, t) for t in table]
        print("    &[" + ", ".join(str(t) for t in table) + ", u16::MAX]")
        print("\nCompare against the current table before adopting it:")
        print("    &[2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 24, 28, u16::MAX]")
        if raw_at_s9 < 15:
            print("\nNote: S9 lands below raw 15, which is where the CAT")
            print("reference says the scale ends. That supports halving the")
            print("range as well as the table -- see item 48.")
        out = {
            "points": [{"raw": r, "panel_db_over_s0": d} for r, d in points],
            "db_per_raw_count": round(db_per_count, 3),
            "raw_counts_per_s_unit": round(counts_per_s, 3),
            "raw_at_s9": round(raw_at_s9, 2),
            "suggested_thresholds": table,
        }
        path = "/tmp/smeter-anchor.json"
        with open(path, "w") as f:
            json.dump(out, f, indent=1)
        print(f"\nsaved to {path}")
        return 0
    finally:
        # Including on Ctrl-C. Leaving an operator's attenuator in is a
        # nasty thing to do to the next person who tunes the band.
        for q, v in before.items():
            if v.endswith(";") and len(v) > 2:
                cat.set(v)
        print("\nradio restored to:", " ".join(before.values()))


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(1)
