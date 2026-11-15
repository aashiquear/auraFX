# A scene exported as a web page (File > Export Scene, .html) and opened in a browser: the
# picture is the browser's own (headless Chrome draws the page).
import aurafx as afx, os, subprocess, tempfile, time
w = afx.wavelet()
c = w.contour(array="RTData", values=[160, 200, 240])
c.set(color_by="RTData", cmap="Turbo")
s = w.slice(normal=[0, 0, 1])
s.set(color_by="RTData", cmap="Turbo")
w.hide()
afx.view("iso"); afx.reset_camera()
page = os.path.join(tempfile.mkdtemp(), "scene.html")
afx.export_scene(page)
for _ in range(100):
    if os.path.exists(page) and os.path.getsize(page) > 1000:
        break
    time.sleep(0.1)
subprocess.run(["google-chrome", "--headless=new", "--no-sandbox", "--use-angle=swiftshader", "--enable-unsafe-swiftshader",
                "--window-size=1100,700", "--virtual-time-budget=4000", "--screenshot=" + OUT, "file://" + page], check=False)
# afx.screenshot is not used: the picture comes from the browser.
