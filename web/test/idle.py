"""Count browser refreshes that draw into the game canvas on an idle screen."""

import argparse
import pathlib

from smoke import opened, sync_playwright, wait_console


COUNT_FRAMES = """() => {
    const raf = window.requestAnimationFrame.bind(window);
    const draws = new Set();
    let refresh = null;
    let betweenRefreshes = 0;
    let total = 0;
    function drew() {
        ++total;
        if (refresh === null) ++betweenRefreshes;
        else draws.add(refresh);
    }
    window.requestAnimationFrame = callback => raf(time => {
        refresh = time;
        try { callback(time); } finally { refresh = null; }
    });
    window.gameDraws = () => total;
    window.observeFrames = async count => {
        draws.clear();
        betweenRefreshes = 0;
        for (let i = 0; i < count; ++i) await new Promise(raf);
        return { refreshesThatDrew: draws.size, betweenRefreshes };
    };
    const gl = WebGL2RenderingContext.prototype;
    const targets = new WeakMap();
    const bind = gl.bindFramebuffer;
    gl.bindFramebuffer = function (target, buffer) {
        bind.call(this, target, buffer);
        if (target === this.FRAMEBUFFER || target === this.DRAW_FRAMEBUFFER) targets.set(this, buffer);
    };
    // Draws to another framebuffer are offscreen passes. The canvas receives only
    // calls targeting the default draw framebuffer.
    for (const method of ['blitFramebuffer', 'clear', 'drawArrays', 'drawElements',
                          'drawArraysInstanced', 'drawElementsInstanced']) {
        const draw = gl[method];
        gl[method] = function (...args) {
            draw.apply(this, args);
            if (this.canvas.id === 'dbota' && !targets.get(this)) drew();
        };
    }
    if (!globalThis.GPUCanvasContext) return;
    const acquire = GPUCanvasContext.prototype.getCurrentTexture;
    const gameTextures = new WeakSet();
    const gameViews = new WeakSet();
    const gameEncoders = new WeakSet();
    const gameCommands = new WeakSet();
    GPUCanvasContext.prototype.getCurrentTexture = function () {
        const texture = acquire.call(this);
        if (this.canvas.id === 'dbota') gameTextures.add(texture);
        return texture;
    };
    const view = GPUTexture.prototype.createView;
    GPUTexture.prototype.createView = function (...args) {
        const result = view.apply(this, args);
        if (gameTextures.has(this)) gameViews.add(result);
        return result;
    };
    const beginPass = GPUCommandEncoder.prototype.beginRenderPass;
    GPUCommandEncoder.prototype.beginRenderPass = function (description) {
        if (description.colorAttachments.some(color =>
            color && (gameViews.has(color.view) || gameViews.has(color.resolveTarget)))) {
            gameEncoders.add(this);
        }
        return beginPass.call(this, description);
    };
    const finish = GPUCommandEncoder.prototype.finish;
    GPUCommandEncoder.prototype.finish = function (...args) {
        const commands = finish.apply(this, args);
        if (gameEncoders.has(this)) gameCommands.add(commands);
        return commands;
    };
    const submit = GPUQueue.prototype.submit;
    GPUQueue.prototype.submit = function (commands) {
        submit.call(this, commands);
        if (commands.some(command => gameCommands.has(command))) drew();
    };
}"""


def the_idle_sign_in_stops_presenting_and_redraws_on_resize(directory, api):
    with sync_playwright() as playwright:
        server, base, browser, page, console, problems = opened(playwright, directory)
        try:
            page.add_init_script(f"({COUNT_FRAMES})()")
            page.goto(base + f"?cache=http&gpu={api}")
            wait_console(page, console, f"dbota: drawing with {api}", 60, api)
            wait_console(page, console, "dbota: running", 300, "the sign-in screen")
            page.wait_for_function("gameDraws() > 0")
            # Let the first UI layout and its assets finish. Sample display refreshes even
            # when the app asks for none. The probe also catches draws outside a callback.
            page.evaluate("observeFrames(120)")
            idle = page.evaluate("observeFrames(30)")
            assert idle == {"refreshesThatDrew": 0, "betweenRefreshes": 0}, f"idle sign-in drew: {idle}"
            before_resize = page.evaluate("gameDraws()")
            page.set_viewport_size({"width": 1180, "height": 660})
            page.wait_for_function("before => gameDraws() > before", arg=before_resize)
            page.evaluate("observeFrames(12)")
            idle = page.evaluate("observeFrames(30)")
            assert sum(idle.values()) == 0, f"after resize, idle sign-in drew: {idle}"
            before_key = page.evaluate("gameDraws()")
            page.keyboard.press("a")
            page.wait_for_function("before => gameDraws() > before", arg=before_key)
            page.evaluate("observeFrames(12)")
            idle = page.evaluate("observeFrames(30)")
            assert sum(idle.values()) == 0, f"after typing, idle sign-in drew: {idle}"
            assert not problems, problems
            print(f"{api}: idle sign-in drew zero frames per 30 refreshes and redrew on resize and typing")
        finally:
            browser.close()
            server.shutdown()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("page", type=pathlib.Path)
    parser.add_argument("--api", choices=("webgpu", "webgl2"), action="append")
    args = parser.parse_args()
    for api in args.api or ("webgpu", "webgl2"):
        the_idle_sign_in_stops_presenting_and_redraws_on_resize(args.page, api)
