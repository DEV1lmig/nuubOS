################################################################################
#
# labwc
#
################################################################################

LABWC_VERSION = 0.20.2
LABWC_SITE = $(call github,labwc,labwc,$(LABWC_VERSION))
LABWC_LICENSE = GPL-2.0+
LABWC_LICENSE_FILES = LICENSE
LABWC_DEPENDENCIES = cairo libxml2 pango wayland wlroots xkeyboard-config libpng
LABWC_CONF_OPTS =     -Dman-pages=disabled     -Dxwayland=disabled     -Dsvg=disabled     -Dicon=disabled     -Dlabnag=disabled     -Dnls=disabled     -Dsystemd-session=disabled     -Dtest=disabled

# nuubOS: the first frame draws the lifecycle splash with the shared
# renderer of nuubos-splash (patch 0001). Copied before every build so a
# labwc-rebuild picks up renderer changes.
define LABWC_NUUBOS_SPLASH_RENDERER
	$(INSTALL) -m 0644 $(BR2_EXTERNAL_NUUBOS_PATH)/package/nuubos/nuubos-splash/src/nuubos-splash-render.h \
		$(@D)/src/nuubos-splash-render.h
endef
LABWC_PRE_BUILD_HOOKS += LABWC_NUUBOS_SPLASH_RENDERER

$(eval $(meson-package))
