/*
 * nuubOS bezel, written for nuubOS (PolyForm Noncommercial 1.0.0, LICENSING.md).
 *
 * The game keeps exactly the size and place it has without bezels (the
 * same rule as RetroArch's smart integer scaling, axis Y, with the core's
 * aspect: integer vertical scale, fit to the screen when that would leave
 * more than 12 % unused, centred horizontally, at the top); the bezel only
 * fills the black around it. RetroArch's viewport is the whole screen
 * (aspect "full", no integer scaling) and this pass does the placement.
 *
 * Look: the matte surface of the user's Mood tinted with the system
 * colour, a thin lip around the picture and the Mood's accent line under
 * it. Side panels wide enough show the platform's silhouette (SYSICON,
 * the system icon of the library, CC BY 4.0) on the left and the nuubOS
 * logo (the dot-matrix wordmark, "OS" in the Mood's accent) standing
 * vertically on the right (the face-button dots when even that does not
 * fit); a bottom strip shows the silhouette and the logo, horizontal, at
 * its ends. BRAND is the logo's dot map (37x7, red = "nuub", green =
 * "OS"); the round dots are drawn here at whole-pixel pitch, crisp at any
 * size. MOOD_* are the Mood colours (splash palette, from emud).
 * BEZEL_STYLE 1 (handhelds) turns the surface into the console's plastic
 * body.
 *
 * BEZEL_R/G/B is the system colour (systems.conf), HAS_ICON 0 when the
 * system has no icon; CRT 1 adds the nuubOS CRT look (crt.glsl) to the
 * picture (bezel-picture.glsl).
 *
 * Two passes. The bezel does not move during a game, but drawing it is
 * far more work than the picture: at 720x720 (RG CubeXX) drawing it every
 * frame cost a quarter of the frame rate (SNES 48 fps, N64 27). This
 * first pass draws only the bezel, into an FBO of the screen's size that
 * feeds back into the next frame (preset: scale_type0 viewport,
 * feedback_pass 0): a bezel pixel is drawn once and then read back. Its
 * alpha holds a key of the picture rectangle, so a geometry change (a PS1
 * game switching resolution) draws the bezel again at once; every frame
 * also redraws one row in 16, which heals the rare key collision within
 * 16 frames. Rows are addressed by gl_FragCoord with row 0 = top of the
 * screen whatever the core (FBO passes are drawn the way the core's frame
 * runs); bezel-picture.glsl, the last pass, reads them back the same way
 * and draws the picture itself. The placement is worked out per vertex.
 */

#pragma parameter BEZEL_R "Bezel red" 0.40 0.0 1.0 0.01
#pragma parameter BEZEL_G "Bezel green" 0.50 0.0 1.0 0.01
#pragma parameter BEZEL_B "Bezel blue" 0.90 0.0 1.0 0.01
#pragma parameter BEZEL_STYLE "Bezel style (0 TV, 1 handheld)" 0.0 0.0 1.0 1.0
#pragma parameter HAS_ICON "System icon available" 0.0 0.0 1.0 1.0
#pragma parameter OVERSCAN_X "Crop the sides to fill the height (max % of the width)" 0.0 0.0 49.0 1.0
#pragma parameter MOOD_BG_R "Mood background red" 0.051 0.0 1.0 0.01
#pragma parameter MOOD_BG_G "Mood background green" 0.055 0.0 1.0 0.01
#pragma parameter MOOD_BG_B "Mood background blue" 0.063 0.0 1.0 0.01
#pragma parameter MOOD_TEXT_R "Mood text red" 0.961 0.0 1.0 0.01
#pragma parameter MOOD_TEXT_G "Mood text green" 0.965 0.0 1.0 0.01
#pragma parameter MOOD_TEXT_B "Mood text blue" 0.973 0.0 1.0 0.01
#pragma parameter MOOD_ACCENT_R "Mood accent red" 0.227 0.0 1.0 0.01
#pragma parameter MOOD_ACCENT_G "Mood accent green" 0.525 0.0 1.0 0.01
#pragma parameter MOOD_ACCENT_B "Mood accent blue" 1.0 0.0 1.0 0.01

#if defined(VERTEX)

#if __VERSION__ >= 130
#define COMPAT_VARYING out
#define COMPAT_ATTRIBUTE in
#else
#define COMPAT_VARYING varying
#define COMPAT_ATTRIBUTE attribute
#endif

COMPAT_ATTRIBUTE vec4 VertexCoord;
COMPAT_ATTRIBUTE vec4 TexCoord;
COMPAT_VARYING vec4 vPic;
COMPAT_VARYING float vKey;

uniform mat4 MVPMatrix;
uniform vec2 InputSize;
uniform vec2 OutputSize;
uniform float OriginalAspect;
#ifdef PARAMETER_UNIFORM
uniform float OVERSCAN_X;
#else
#define OVERSCAN_X 0.0
#endif

/* Picture rectangle (xy origin, zw size) exactly as RetroArch places it
 * without bezels: video_viewport_get_scaled_integer, smart mode, axis Y,
 * plus nuubOS patch 0003 (overscan cropped evenly) and bias x 0.5, y 0. */
vec4 placement(vec2 screen)
{
    float aspect = OriginalAspect > 0.0 ? OriginalAspect : InputSize.x / InputSize.y;
    vec2 one = vec2(floor(InputSize.y * aspect + 0.5), InputSize.y);
    vec2 size;
    float wide = floor(screen.y * aspect + 0.5);
    if (OVERSCAN_X > 0.5 && wide > screen.x) {
        /* TV systems on a square screen (RetroArch patch 0006): the full
         * height as far as at most OVERSCAN_X % of the width is cropped. */
        float keep = 1.0 - OVERSCAN_X / 100.0;
        size = vec2(wide, screen.y);
        if (wide * keep > screen.x) {
            size.x = floor(screen.x / keep + 0.5);
            size.y = floor(size.x / aspect + 0.5);
        }
    } else if (one.x > screen.x || one.y > screen.y) {
        size = vec2(0.0);
    } else {
        float under = floor(screen.y / one.y);
        float min_h = one.y - 6.0;
        if (one.y >= 192.0 && one.y <= 240.0)
            min_h = 192.0;
        else if (one.y >= 384.0 && one.y <= 480.0)
            min_h = 384.0;
        float sh = floor(screen.y / (under + 1.0)) >= min_h ? under + 1.0 : under;
        float s = min(floor(screen.x / one.x), sh);
        size = one * s;
        vec2 use = size / screen;
        if (max(use.x, use.y) < 0.88)
            size = vec2(0.0);
    }
    if (size.x == 0.0) {
        /* Fit with the aspect (video_viewport_get_scaled_aspect). */
        size = screen.x / screen.y > aspect
            ? vec2(floor(screen.y * aspect + 0.5), screen.y)
            : vec2(screen.x, floor(screen.x / aspect + 0.5));
    }
    /* RetroArch truncates the centring offset towards zero. */
    float pad = screen.x - size.x;
    vec2 origin = vec2(pad < 0.0 ? -floor(-pad * 0.5) : floor(pad * 0.5),
                       size.y > screen.y ? floor((screen.y - size.y) * 0.5) : 0.0);
    return vec4(origin, size);
}

void main()
{
    gl_Position = MVPMatrix * VertexCoord;
    vPic = placement(OutputSize);
    /* 1..254, never the 0 of a cleared feedback texture. */
    vKey = (mod(dot(vPic, vec4(7.0, 13.0, 3.0, 5.0)) + dot(OutputSize, vec2(1.0, 11.0)),
                254.0) + 1.0) / 255.0;
}

#elif defined(FRAGMENT)

#ifdef GL_ES
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#endif

#if __VERSION__ >= 130
#define COMPAT_VARYING in
#define COMPAT_TEXTURE texture
out vec4 FragColor;
#else
#define COMPAT_VARYING varying
#define FragColor gl_FragColor
#define COMPAT_TEXTURE texture2D
#endif

uniform vec2 OutputSize;
uniform int FrameCount;
uniform sampler2D FeedbackTexture;
uniform vec2 FeedbackTextureSize;
uniform sampler2D SYSICON;
uniform sampler2D BRAND;
COMPAT_VARYING vec4 vPic;
COMPAT_VARYING float vKey;

#ifdef PARAMETER_UNIFORM
uniform float BEZEL_R;
uniform float BEZEL_G;
uniform float BEZEL_B;
uniform float BEZEL_STYLE;
uniform float HAS_ICON;
uniform float MOOD_BG_R, MOOD_BG_G, MOOD_BG_B;
uniform float MOOD_TEXT_R, MOOD_TEXT_G, MOOD_TEXT_B;
uniform float MOOD_ACCENT_R, MOOD_ACCENT_G, MOOD_ACCENT_B;
#else
#define BEZEL_R 0.40
#define BEZEL_G 0.50
#define BEZEL_B 0.90
#define BEZEL_STYLE 0.0
#define HAS_ICON 0.0
#define MOOD_BG_R 0.051
#define MOOD_BG_G 0.055
#define MOOD_BG_B 0.063
#define MOOD_TEXT_R 0.961
#define MOOD_TEXT_G 0.965
#define MOOD_TEXT_B 0.973
#define MOOD_ACCENT_R 0.227
#define MOOD_ACCENT_G 0.525
#define MOOD_ACCENT_B 1.0
#endif

/* The logo grid (nuubos-splash-render.h): 37 x 7 dots. */
const vec2 LOGO_GRID = vec2(37.0, 7.0);

/* Signed distance to a rounded box of half size b. */
float round_box(vec2 p, vec2 b, float r)
{
    vec2 q = abs(p) - b + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

/* Coverage (0..1) of an image of size box drawn at origin o, by its alpha. */
float stamp(sampler2D tex, vec2 p, vec2 o, vec2 box)
{
    vec2 uv = (p - o) / box;
    if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0)
        return 0.0;
    return COMPAT_TEXTURE(tex, uv).a;
}

/* The logo at whole-pixel dot pitch k; q = position in logo pixels (x
 * along the word, y down the letters). rgb = colour, a = coverage. */
vec4 logo(vec2 q, float k, vec3 text, vec3 accent)
{
    vec2 cell = floor(q / k);
    if (cell.x < 0.0 || cell.y < 0.0 || cell.x >= LOGO_GRID.x || cell.y >= LOGO_GRID.y)
        return vec4(0.0);
    vec4 t = COMPAT_TEXTURE(BRAND, (cell + 0.5) / LOGO_GRID);
    if (t.a < 0.5)
        return vec4(0.0);
    float s = k - max(1.0, floor(k / 6.0));
    float d = round_box(q - cell * k - vec2(s * 0.5), vec2(s * 0.5), s * 0.25);
    return vec4(mix(text, accent, t.g), clamp(0.5 - d, 0.0, 1.0));
}

/* The four nuubOS face-button dots (the hint glyph), south one filled. */
float dots(vec2 p, vec2 c, float r, float aa, out float filled)
{
    float g = r * 2.6;
    vec2 q = p - c;
    float best = 1e4;
    float south = length(q - vec2(0.0, g));
    best = min(best, abs(length(q - vec2(0.0, -g)) - r * 0.78));
    best = min(best, abs(length(q - vec2(-g, 0.0)) - r * 0.78));
    best = min(best, abs(length(q - vec2(g, 0.0)) - r * 0.78));
    filled = 1.0 - smoothstep(r - aa, r + aa, south);
    return 1.0 - smoothstep(r * 0.22 - aa, r * 0.22 + aa, best);
}

void main()
{
    /* Output pixel, origin top-left. */
    vec2 p = gl_FragCoord.xy;
    /* The same value at every vertex: rounding undoes interpolation. */
    vec4 pic = floor(vPic + 0.5);
    float key = floor(vKey * 255.0 + 0.5) / 255.0;
    vec2 origin = pic.xy;
    vec2 size = pic.zw;
    vec2 local = p - origin;

    if (local.x >= 0.0 && local.y >= 0.0 && local.x < size.x && local.y < size.y) {
        FragColor = vec4(0.0); /* the picture: bezel-picture.glsl */
        return;
    }

    /* Bezel drawn in an earlier frame for the same picture rectangle. */
    if (mod(floor(gl_FragCoord.y) + float(FrameCount), 16.0) >= 0.5) {
        vec4 last = COMPAT_TEXTURE(FeedbackTexture, gl_FragCoord.xy / FeedbackTextureSize);
        if (abs(last.a - key) < 0.5 / 255.0) {
            FragColor = last;
            return;
        }
    }

    /* Bezel. u = one 480p pixel, so every screen gets the same look. */
    float u = OutputSize.y / 480.0;
    float aa = 0.75;
    vec3 sys = vec3(BEZEL_R, BEZEL_G, BEZEL_B);
    bool handheld = BEZEL_STYLE > 0.5;
    vec2 centre = origin + size * 0.5;
    float left = max(origin.x, 0.0);
    float right = max(OutputSize.x - origin.x - size.x, 0.0);
    float bottom = max(OutputSize.y - origin.y - size.y, 0.0);

    /* Surface: nuubOS matte tinted by the system (TV), or the console's
     * plastic body (handheld); light from above, soft vignette. */
    vec2 uv = p / OutputSize - 0.5;
    vec3 mood_bg = vec3(MOOD_BG_R, MOOD_BG_G, MOOD_BG_B);
    vec3 mood_text = vec3(MOOD_TEXT_R, MOOD_TEXT_G, MOOD_TEXT_B);
    vec3 mood_accent = vec3(MOOD_ACCENT_R, MOOD_ACCENT_G, MOOD_ACCENT_B);
    vec3 col = handheld ? mix(vec3(0.17, 0.18, 0.19), sys, 0.34)
                        : mix(mood_bg, sys, 0.14);
    col *= 1.06 - 0.16 * (p.y / OutputSize.y) - 0.35 * dot(uv, uv);

    /* Lip around the picture: a darker recess, softly shadowed. */
    float d = round_box(p - centre, size * 0.5, 2.0 * u);
    float lip = (handheld ? 5.0 : 3.0) * u;
    col = mix(col * 0.45, col, smoothstep(lip, lip + 6.0 * u, d));
    col = mix(col, vec3(0.02), 1.0 - smoothstep(0.0, lip, d));

    /* The Mood's accent line under the picture (the splash track). */
    float under = p.y - (origin.y + size.y);
    if (bottom >= 6.0 * u && under >= 0.0) {
        float line = 1.0 - smoothstep(1.5 * u - aa, 1.5 * u + aa, abs(under - 3.0 * u));
        float span = 1.0 - smoothstep(size.x * 0.5 - 1.0, size.x * 0.5 + 1.0, abs(p.x - centre.x));
        col = mix(col, mood_accent, line * span);
    }

    vec3 ink = mix(sys, vec3(1.0), 0.30);
    float panel = min(left, right);
    bool sides = panel >= 56.0 * u;
    bool strip = bottom >= 26.0 * u;
    float mid = origin.y + size.y * 0.5;
    if (sides && p.y < origin.y + size.y) {
        /* Side panels: platform silhouette left, the logo standing
         * upright right (the face-button dots when it does not fit). */
        if (p.x < left) {
            float box = min(panel * 0.86, size.y * 0.62);
            if (HAS_ICON > 0.5) {
                vec2 o = vec2((left - box) * 0.5, mid - box * 0.5);
                col = mix(col, ink, 0.92 * stamp(SYSICON, p, o, vec2(box)));
            }
        } else {
            /* The logo standing on the right panel, reading upwards. */
            float x0 = OutputSize.x - right;
            float k = floor(min((right - 24.0 * u) / LOGO_GRID.y, size.y * 0.72 / LOGO_GRID.x));
            if (k >= 2.0) {
                vec2 box = vec2(LOGO_GRID.y, LOGO_GRID.x) * k;
                vec2 o = vec2(x0 + floor((right - box.x) * 0.5), floor(mid - box.y * 0.5));
                vec4 w = logo(vec2(o.y + box.y - p.y, p.x - o.x), k, mood_text, mood_accent);
                col = mix(col, w.rgb, w.a);
            } else {
                float filled;
                float r = min(right * 0.085, 10.0 * u);
                float ring = dots(p, vec2(x0 + right * 0.5, mid), r, aa, filled);
                col = mix(col, ink * 0.85, ring);
                col = mix(col, mood_accent, filled);
            }
        }
    } else if (strip && p.y > origin.y + size.y) {
        /* Bottom strip: silhouette at the left end (unless the side panel
         * already shows it), the logo at the right end. */
        float y0 = origin.y + size.y + 6.0 * u;
        float h = bottom - 12.0 * u;
        if (HAS_ICON > 0.5 && !sides) {
            float box = min(h, 64.0 * u);
            vec2 o = vec2(left + 10.0 * u, y0 + (h - box) * 0.5);
            col = mix(col, ink, 0.92 * stamp(SYSICON, p, o, vec2(box)));
        }
        float k = max(floor(min(h * 0.5, 28.0 * u) / LOGO_GRID.y), 2.0);
        vec2 box = LOGO_GRID * k;
        vec2 o = vec2(floor(OutputSize.x - right - 10.0 * u - box.x), floor(y0 + (h - box.y) * 0.5));
        vec4 w = logo(p - o, k, mood_text, mood_accent);
        col = mix(col, w.rgb, w.a);
    }

    FragColor = vec4(col, key);
}

#endif
