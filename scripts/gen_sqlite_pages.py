"""An SQLite file that exercises the reader's b-tree code: testdata/vectors/pages.sqlite.

    python3 scripts/gen_sqlite_pages.py

Page size 512 (so 3000 rows need interior pages), an INTEGER PRIMARY KEY, text, reals,
negative and large integers, NULLs and some 3000-byte blobs (overflow pages).
"""
import os, sqlite3
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "vectors", "pages.sqlite")
if os.path.exists(out):
    os.remove(out)
c = sqlite3.connect(out)
c.execute("PRAGMA page_size = 512")
c.execute('CREATE TABLE "t" ( "id" INTEGER PRIMARY KEY, "name" TEXT, "x" REAL, "big" INTEGER, "data" BLOB)')
rows = []
for i in range(1, 3001):
    blob = bytes((i + k) % 256 for k in range(3000)) if i % 150 == 0 else None
    rows.append((i, f"row {i}" if i % 7 else None, i * 0.5, -i * 1_000_000_007 if i % 2 else i, blob))
c.executemany("INSERT INTO t VALUES (?, ?, ?, ?, ?)", rows)
c.commit()
c.execute("VACUUM")
c.close()
print(out, os.path.getsize(out))
