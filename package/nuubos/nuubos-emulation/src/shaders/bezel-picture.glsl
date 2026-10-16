/*
 * nuubOS bezel, last pass, written for nuubOS (PolyForm Noncommercial 1.0.0, LICENSING.md).
 *
 * Draws the picture from the core's frame (OrigTexture) where RetroArch
 * would place it without bezels and, around it, the bezel the first pass
 * (bezel.glsl) left in its FBO, read back by screen position (FBO row 0 =
 * top of the screen, the first pass's convention). CRT 1 gives the picture the nuubOS CRT look (crt.glsl);
 * otherwise it is sharp bilinear: one texture fetch at an integer scale,
 * up to four where a non-integer scale blends, two with CRT.
 */

#pragma parameter CRT "CRT look" 0.0 0.0 1.0 1.0
/* OVERSCAN_X is declared by bezel.glsl only: RetroArch keeps one entry per
 * pragma and pass, sets the preset value on the first and uploads them all
 * by name, so a second declaration would reset the uniform to 0. */

#if defined(VERTEX)

#if __VERSION__ >= 130
#define COMPAT_VARYING out
#define COMPAT_ATTRIBUTE in
#else
#define COMPAT_VARYING varying
#define COMPAT_ATTRIBUTE attribute
#endif

COMPAT_ATTRIBUTE vec4 VertexCoord;
COMPAT_ATTRIBUTE vec4 OrigTexCoord;
COMPAT_VARYING vec2 vScreen;
COMPAT_VARYING float vRowsDown;
COMPAT_VARYING vec4 vPic;

uniform mat4 MVPMatrix;
uniform vec2 OutputSize;
uniform vec2 OrigTextureSize;
uniform vec2 OrigInputSize;
uniform float OriginalAspect;
#ifdef PARAMETER_UNIFORM
uniform float OVERSCAN_X;
#else
#define OVERSCAN_X 0.0
#endif

/* Picture rectangle, the same as placement() in bezel.glsl (there with
 * InputSize, the core's frame in that pass). */
vec4 placement(vec2 screen)
{
    float aspect = OriginalAspect > 0.0 ? OriginalAspect : OrigInputSize.x / OrigInputSize.y;
    vec2 one = vec2(floor(OrigInputSize.y * aspect + 0.5), OrigInputSize.y);
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

/* RetroArch hands a hardware rendered core's frame (PSP, N64) bottom row
 * first and a software one (GBA) top row first, flipping the texture
 * coordinates to match: placement uses the screen position (clip space:
 * this pass draws on the screen, the preset gives it no scale, else
 * RetroArch would add a pass of its own after it) and the corners tell
 * which way rows run. */
void main()
{
    gl_Position = MVPMatrix * VertexCoord;
    vScreen = vec2(gl_Position.x + 1.0, 1.0 - gl_Position.y) * 0.5;
    bool tex_low = OrigTexCoord.y * OrigTextureSize.y < 0.5 * OrigInputSize.y;
    vRowsDown = (tex_low == (vScreen.y < 0.5)) ? 1.0 : 0.0;
    vPic = placement(OutputSize);
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
uniform vec2 TextureSize;
uniform sampler2D Texture;
uniform vec2 OrigTextureSize;
uniform vec2 OrigInputSize;
uniform sampler2D OrigTexture;
COMPAT_VARYING vec2 vScreen;
COMPAT_VARYING float vRowsDown;
COMPAT_VARYING vec4 vPic;

#ifdef PARAMETER_UNIFORM
uniform float CRT;
#else
#define CRT 0.0
#endif

/* Texel t of the picture (origin top-left, whichever way rows run). */
vec3 texel(vec2 t)
{
    t = clamp(t, vec2(0.0), OrigInputSize - 1.0);
    if (vRowsDown < 0.5)
        t.y = OrigInputSize.y - 1.0 - t.y;
    return COMPAT_TEXTURE(OrigTexture, (t + 0.5) / OrigTextureSize).rgb;
}

/* The picture at source position src (texels from its top-left), scaled
 * by sc per axis: sharp bilinear (hard pixels, a one output pixel blend
 * only where a non-integer scale would make them uneven), or the CRT look
 * when enabled. */
vec3 picture(vec2 src, vec2 sc)
{
    vec2 base = floor(src);
    vec2 f = src - base;
    if (CRT < 0.5) {
        vec2 k = max(floor(sc + 0.01), vec2(1.0));
        if (abs(k.x - sc.x) < 0.01 && abs(k.y - sc.y) < 0.01)
            return texel(base);
        vec2 off = f - 0.5;
        vec2 edge = 0.5 - 0.5 / k;
        vec2 b = (off - clamp(off, -edge, edge)) * k;
        vec2 dir = sign(b);
        vec2 w = abs(b);
        vec3 top = mix(texel(base), texel(base + vec2(dir.x, 0.0)), w.x);
        vec3 bottom = mix(texel(base + vec2(0.0, dir.y)), texel(base + dir), w.x);
        return mix(top, bottom, w.y);
    }

    float s = sc.y;
    float tx = f.x - 0.5;
    float side = tx < 0.0 ? -1.0 : 1.0;
    float melt = 0.5 * smoothstep(0.2, 0.5, abs(tx));
    vec3 col = mix(texel(base), texel(base + vec2(side, 0.0)), melt);
    /* At a whole scale every source line gets the same rows (beam then
     * gap); at a fractional one (4:3 on a square screen, 2.45x) lines get
     * 2 or 3 rows: the pixel's own position is sampled. Light scanlines
     * everywhere, as crt.glsl. */
    bool whole = abs(s - floor(s + 0.5)) < 0.01;
    float centre = whole ? 0.5 - 0.5 / s : 0.5;
    float row = whole ? (floor(f.y * s) + 0.5) / s : f.y;
    float dy = row - centre;
    float width = mix(0.20, 0.34, dot(col, vec3(0.299, 0.587, 0.114)));
    float strength = s >= 2.0 ? 0.25 : 0.0;
    float scan = 1.0 - strength * (1.0 - exp(-(dy * dy) / (2.0 * width * width)));
    return min(col * scan * (1.0 + 0.55 * strength), vec3(1.0));
}

void main()
{
    /* Output pixel, origin top-left. */
    vec2 p = vScreen * OutputSize;
    /* The same value at every vertex: rounding undoes interpolation. */
    vec4 pic = floor(vPic + 0.5);
    vec2 local = p - pic.xy;

    if (local.x >= 0.0 && local.y >= 0.0 && local.x < pic.z && local.y < pic.w)
        FragColor = vec4(picture(local / pic.zw * OrigInputSize, pic.zw / OrigInputSize), 1.0);
    else
        FragColor = vec4(COMPAT_TEXTURE(Texture, (floor(p) + 0.5) / TextureSize).rgb, 1.0);
}

#endif
