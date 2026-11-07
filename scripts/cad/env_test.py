# Drives the CAD environment inside the running app (aurafx --script) and writes PASS or the
# failure to the file named by RESULT; scripts/cad/env_test.sh runs it.
import aurafx as afx
try:
    m = afx.cad_model()
    m.update()
    n0 = len(m.cad.faces())
    e = m.edit_cad()
    assert e.editing()
    e.box(size=(10, 10, 6), name="Boss", at=(0, 0, 11))            # adds five faces
    e.hole(diameter=4, depth=4, name="Pocket", at=(0, 0, 14))      # a blind hole in the boss: two more
    info = e.evaluate()
    n1 = len(info["faces"])
    assert n1 == n0 + 7, (n0, n1)
    r = e.apply()
    assert not e.editing()
    m.update()
    n2 = len(m.cad.faces())
    assert n2 == n1, (n1, n2)
    open(RESULT, "w").write("PASS faces %d -> %d" % (n0, n2))
except Exception as ex:
    open(RESULT, "w").write("FAIL %r" % (ex,))
