/*
 * nuubOS CRT, written for nuubOS (PolyForm Noncommercial 1.0.0, LICENSING.md).
 *
 * A very light CRT look for systems made for TV screens, on top of the
 * pixel perfect (integer scaled) picture: neighbouring pixels melt into
 * each other horizontally the way a composite/RF signal blurred them, and
 * every source line becomes a beam whose height grows with its brightness
 * (dark gaps between dark lines, bright lines blooming). A TV traced at
 * most ~288 lines a field, so taller sources (a core's 2x internal
 * resolution such as PS1 enhancement, 480i/576i) get one beam per 2 rows
 * (3 above 576) while the colour keeps every row. Below 2x vertical scale
 * of those beam lines there is no room for scanlines and only the
 * horizontal blend is applied. Single pass, two texture fetches:
 * negligible on Mali-G31.
 */

#pragma parameter CRT_SCANLINES "Scanline strength" 0.25 0.0 1.0 0.05
#pragma parameter CRT_BLEND "Horizontal blend" 0.30 0.0 0.5 0.05

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
COMPAT_VARYING vec2 vTexCoord;

uniform mat4 MVPMatrix;

void main()
{
    gl_Position = MVPMatrix * VertexCoord;
    vTexCoord = TexCoord.xy;
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
uniform vec2 InputSize;
uniform sampler2D Texture;
COMPAT_VARYING vec2 vTexCoord;

#ifdef PARAMETER_UNIFORM
uniform float CRT_SCANLINES;
uniform float CRT_BLEND;
#else
#define CRT_SCANLINES 0.25
#define CRT_BLEND 0.30
#endif

void main()
{
    vec2 texel = vTexCoord * TextureSize;
    /* Source rows per beam line: 1 up to 288 lines, 2 up to 576, ... (PS1
     * at 2x internal resolution lost its scanlines, user report
     * 2026-10-18). */
    float k = floor((InputSize.y - 1.0) / 288.0) + 1.0;
    float s = OutputSize.y * k / InputSize.y;
    /* A fractional vertical scale (4:3 on a square screen) gives lines 2
     * or 3 rows: sample the pixel's own position. */
    bool whole = abs(s - floor(s + 0.5)) < 0.01;
    vec2 base = floor(texel);
    vec2 f = texel - base;
    float fl = fract(texel.y / k);

    /* Horizontal melt: towards the neighbour on the near side, only in the
     * outer CRT_BLEND of the pixel, at most half way. */
    float tx = f.x - 0.5;
    float side = tx < 0.0 ? -1.0 : 1.0;
    float melt = 0.5 * smoothstep(0.5 - CRT_BLEND, 0.5, abs(tx));
    vec3 c0 = COMPAT_TEXTURE(Texture, (base + 0.5) / TextureSize).rgb;
    vec3 c1 = COMPAT_TEXTURE(Texture, (base + vec2(side, 0.0) + 0.5) / TextureSize).rgb;
    vec3 col = mix(c0, c1, melt);

    /* Beam: centred half an output row above the pixel centre, so at 2x
     * the first row is the beam and the second the gap. */
    float centre = whole ? 0.5 - 0.5 / s : 0.5;
    float row = whole ? (floor(fl * s) + 0.5) / s : fl;
    float dy = row - centre;
    float lum = dot(col, vec3(0.299, 0.587, 0.114));
    float width = mix(0.20, 0.34, lum);
    float beam = exp(-(dy * dy) / (2.0 * width * width));
    /* Light scanlines everywhere (user request 2026-10-11: 0.55 read as
     * dark lines, on N64 and on fractional scales most). */
    float strength = s >= 2.0 ? CRT_SCANLINES : 0.0;
    float scan = 1.0 - strength * (1.0 - beam);

    /* Gain back the light the gaps take: at 2x a gap row keeps ~0.5, so
     * mid tones average the original brightness (user report 2026-10-09:
     * the CRT look darkened the picture; was 0.30). Whites clip. */
    col *= scan * (1.0 + 0.55 * strength);
    FragColor = vec4(min(col, vec3(1.0)), 1.0);
}

#endif
