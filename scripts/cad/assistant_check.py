# Asks the AI assistant, inside the running app (aurafx --script), to start a CAD model, to build
# a part in it, to edit the part and to change it in the CAD environment, and checks the part
# after each request. Writes a report to the file named by RESULT (the first line is PASS or FAIL);
# scripts/cad/assistant_check.sh runs it with a model of your choice.
import time
import aurafx as afx

REPORT = []
FAILED = []


def say(text):
    REPORT.append(text)
    # The first line stays RUNNING until the last step (assistant_check.sh waits for it to change).
    open(RESULT, "w").write("RUNNING" + ("  (failed so far: %s)" % ", ".join(FAILED) if FAILED else "") + "\n" + "\n".join(REPORT))


def cad_nodes():
    out = []
    for n in afx.nodes():
        try:
            n.cad.info()
            out.append(n)
        except Exception:
            pass
    return out


def part():
    """The CAD Model the assistant worked on: it may start a second node, so take the one with the most features."""
    nodes = cad_nodes()
    return max(nodes, key=lambda n: len(n.cad.features())) if nodes else None


def built(m):
    """The built part of a node, or None (with the reason in the report) when it does not build."""
    try:
        return m.cad.evaluate()
    except Exception as ex:
        say("the part does not build: %s" % ex)
        return None


def idle(timeout=420, quiet=15):
    """Wait until the assistant has been done (no answer arriving, no agent step pending) for `quiet` seconds."""
    t0, calm = time.time(), None
    while time.time() - t0 < timeout:
        s = afx.assistant_status()
        if s["busy"] or s["agent"]:
            calm = None
        elif calm is None:
            calm = time.time()
        elif time.time() - calm >= quiet:
            return True
        time.sleep(1)
    return False


def ask(step, text):
    say("== %s\nASK: %s" % (step, text))
    n = afx.assistant_status()["messages"]
    afx.ask(text, agent=True)
    t0 = time.time()
    while afx.assistant_status()["messages"] == n and time.time() - t0 < 30:
        time.sleep(0.5)
    done = idle()
    s = afx.assistant_status()
    say("done=%s error=%s messages=%d\nANSWER (end): %s" % (done, s["error"], s["messages"], s["last"][-700:].replace("\n", "\n    ")))
    for role, text in s["history"][-8:]:
        say("  [%s] %s" % (role, text[:600].replace("\n", "\n      ")))
    return done and not s["error"]


def check(step, ok, detail):
    if not ok:
        FAILED.append(step)
    say("%s %s: %s" % ("ok  " if ok else "FAIL", step, detail))


def main():
    try:
        afx.show_panel("Assistant")
        # 1. initiating: a CAD model with named dimensions
        ask("initiate", "Start a new CAD model called Bracket: a base plate 60 mm wide, 40 mm deep and 8 mm thick. "
            "Name the three dimensions width, depth and thickness (parameters) and build the plate from them.")
        cads = cad_nodes()
        check("initiate: a CAD Model exists", len(cads) >= 1, "%d CAD node(s)" % len(cads))
        m = part()
        info = m.cad.info()
        names = set(p["name"] for p in info["parameters"])
        check("initiate: the dimensions are named", {"width", "depth", "thickness"} <= names, "parameters %s" % sorted(names))
        ev = built(m) or {"volume": 0, "faces": [], "bounds": [[0, 0, 0], [0, 0, 0]]}
        vol = ev["volume"]
        check("initiate: the plate is 60 x 40 x 8", abs(vol - 60 * 40 * 8) < 0.03 * 60 * 40 * 8, "volume %.0f (19200 expected)" % vol)

        # 2. generating: more features, built and checked
        ask("generate", "Make the Bracket an L-bracket: add a vertical wall 8 mm thick and 30 mm tall along the back edge of the plate "
            "(full width), and cut two 6 mm holes through the base plate, 12 mm in from each short end and centered between "
            "front and back. Name the features Wall and Hole 1 and Hole 2. Then print the volume and the face names.")
        m = part()
        feats = [f["name"] for f in m.cad.features()]
        check("generate: features were added", len(feats) >= 4, "features %s" % feats)
        ev = built(m) or {"volume": 0, "faces": [], "bounds": [[0, 0, 0], [0, 0, 0]]}
        expect = 60 * 40 * 8 + 60 * 8 * 30 - 2 * 3.14159265 * 9 * 8
        check("generate: the volume is the L-bracket's", abs(ev["volume"] - expect) < 0.06 * expect, "volume %.0f (about %.0f expected)" % (ev["volume"], expect))
        holes = [f["name"] for f in ev["faces"] if "side" in f["name"] and "ole" in f["name"]]
        check("generate: the holes have walls", len(holes) >= 1, "faces %s" % holes)

        # 3. editing: change a dimension and a feature
        ask("edit", "Change the Bracket: the plate should be 80 mm wide and 10 mm thick, and the holes 8 mm in diameter. "
            "Keep everything else and check the new volume.")
        m = part()
        ev = built(m) or {"volume": 0, "faces": [], "bounds": [[0, 0, 0], [0, 0, 0]]}
        x0, x1 = ev["bounds"][0][0], ev["bounds"][1][0]
        check("edit: the plate is 80 mm wide", abs((x1 - x0) - 80) < 1.5, "width %.1f" % (x1 - x0))
        params = m.cad.parameters()
        check("edit: the thickness is 10", abs(float(params.get("thickness", 0)) - 10) < 1e-6 or abs(ev["bounds"][1][2] - ev["bounds"][0][2] - 40) < 1.5, "thickness %s" % params.get("thickness"))

        # 4. in the CAD environment: the same part edited as a working copy and applied
        m = part()
        before = (built(m) or {"volume": 0})["volume"]
        ask("environment", "Open the Bracket in the CAD environment (m.edit_cad()), add a 3 mm fillet to the whole part, "
            "build it with e.evaluate() and apply it with e.apply(). Say what the volume is afterwards.")
        m = part()
        after = (built(m) or {"volume": 0})["volume"]
        check("environment: left the environment applied", not m.cad.editing(), "editing=%s" % m.cad.editing())
        # A fillet of the whole part is a finishing modifier on the document and takes material off the edges.
        check("environment: the fillet is in the part", m.cad.info()["finish"] >= 1 and 0 < before - after < 0.05 * before,
              "finishing modifiers %s, volume %.0f -> %.0f" % (m.cad.info()["finish"], before, after))
        say("final features: %s" % [f["name"] for f in m.cad.features()])
    except Exception as ex:
        FAILED.append("exception")
        say("EXCEPTION %r" % (ex,))
    open(RESULT, "w").write(("FAIL" if FAILED else "PASS") + "\n" + "\n".join(REPORT))


# The assistant's code runs in this same interpreter, so this script must return for it to run: the
# checks go on in a thread, and the script ends at once.
import threading
threading.Thread(target=main, daemon=True).start()
