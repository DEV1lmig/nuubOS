################################################################################
#
# Common settings for the libretro core packages
#
################################################################################

# libretro core Makefiles append their own flags to CC/CFLAGS taken from the
# environment. GCC 15 defaults to C23 and turns several old warnings into
# errors (pointer/int conversions, implicit declarations) that much older
# emulator C code still has, so C is pinned to gnu11 with those kept as
# warnings.
LIBRETRO_CORE_CFLAGS = \
	-std=gnu11 \
	-Wno-error=incompatible-pointer-types \
	-Wno-error=int-conversion \
	-Wno-error=implicit-function-declaration \
	-Wno-error=implicit-int

LIBRETRO_CORE_ENV = \
	$(TARGET_CONFIGURE_OPTS) \
	CFLAGS="$(TARGET_CFLAGS) $(LIBRETRO_CORE_CFLAGS)" \
	CXXFLAGS="$(TARGET_CXXFLAGS)" \
	LDFLAGS="$(TARGET_LDFLAGS) -lm"

LIBRETRO_CORE_MAKE_OPTS = platform=unix

# Cores nuubOS patches for behaviour or performance are bundled under their
# own id, <upstream id>_nuubos ("melonDS-nuubOS"), so the upstream build
# stays available as a download from the buildbot next to them (it would
# otherwise be hidden by a bundled core of the same file name). The core-info
# file is upstream's with the names suffixed: same databases and firmware.
# $(1) upstream id, $(2) bundled id, $(3) built .so
define LIBRETRO_NUUBOS_CORE_INSTALL
	$(INSTALL) -D -m 0755 $(3) $(TARGET_DIR)/usr/lib/libretro/$(2)_libretro.so
	rm -f $(TARGET_DIR)/usr/lib/libretro/$(1)_libretro.so
	sed -e 's/^corename = "\(.*\)"/corename = "\1-nuubOS"/' \
		-e 's/^display_name = "\(.*\))"/display_name = "\1-nuubOS)"/' \
		$(TARGET_DIR)/usr/share/libretro/info/$(1)_libretro.info \
		> $(TARGET_DIR)/usr/share/libretro/info/$(2)_libretro.info
endef
