################################################################################
#
# nuubos-fonts-noto
#
################################################################################

# Fallback glyphs for the UI languages Cantarell lacks (Slint resolves them
# through fontconfig). Upstream release files, unmodified.
NUUBOS_FONTS_NOTO_VERSION = Sans2.004
NUUBOS_FONTS_NOTO_SITE = https://github.com/notofonts/noto-cjk/raw/$(NUUBOS_FONTS_NOTO_VERSION)/Sans/OTC
NUUBOS_FONTS_NOTO_SOURCE = NotoSansCJK-Regular.ttc
NUUBOS_FONTS_NOTO_THAI = NotoSansThai-v2.002
NUUBOS_FONTS_NOTO_DEVANAGARI = NotoSansDevanagari-v2.007
NUUBOS_FONTS_NOTO_EXTRA_DOWNLOADS = \
	$(NUUBOS_FONTS_NOTO_SITE)/NotoSansCJK-Bold.ttc \
	https://github.com/notofonts/noto-cjk/raw/$(NUUBOS_FONTS_NOTO_VERSION)/LICENSE \
	https://github.com/notofonts/thai/releases/download/$(NUUBOS_FONTS_NOTO_THAI)/$(NUUBOS_FONTS_NOTO_THAI).zip \
	https://github.com/notofonts/devanagari/releases/download/$(NUUBOS_FONTS_NOTO_DEVANAGARI)/$(NUUBOS_FONTS_NOTO_DEVANAGARI).zip
NUUBOS_FONTS_NOTO_LICENSE = OFL-1.1
NUUBOS_FONTS_NOTO_LICENSE_FILES = LICENSE-cjk.txt OFL-thai.txt OFL-devanagari.txt
NUUBOS_FONTS_NOTO_REDISTRIBUTE = YES

define NUUBOS_FONTS_NOTO_EXTRACT_CMDS
	cp $(NUUBOS_FONTS_NOTO_DL_DIR)/NotoSansCJK-Regular.ttc $(@D)/
	cp $(NUUBOS_FONTS_NOTO_DL_DIR)/NotoSansCJK-Bold.ttc $(@D)/
	cp $(NUUBOS_FONTS_NOTO_DL_DIR)/LICENSE $(@D)/LICENSE-cjk.txt
	$(UNZIP) -o -d $(@D)/thai $(NUUBOS_FONTS_NOTO_DL_DIR)/$(NUUBOS_FONTS_NOTO_THAI).zip
	$(UNZIP) -o -d $(@D)/devanagari $(NUUBOS_FONTS_NOTO_DL_DIR)/$(NUUBOS_FONTS_NOTO_DEVANAGARI).zip
	cp $(@D)/thai/OFL.txt $(@D)/OFL-thai.txt
	cp $(@D)/devanagari/OFL.txt $(@D)/OFL-devanagari.txt
endef

define NUUBOS_FONTS_NOTO_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0644 $(@D)/NotoSansCJK-Regular.ttc $(TARGET_DIR)/usr/share/fonts/noto/NotoSansCJK-Regular.ttc
	$(INSTALL) -D -m 0644 $(@D)/NotoSansCJK-Bold.ttc $(TARGET_DIR)/usr/share/fonts/noto/NotoSansCJK-Bold.ttc
	$(foreach f,Regular Bold,\
		$(INSTALL) -D -m 0644 $(@D)/thai/NotoSansThai/hinted/ttf/NotoSansThai-$(f).ttf \
			$(TARGET_DIR)/usr/share/fonts/noto/NotoSansThai-$(f).ttf$(sep)\
		$(INSTALL) -D -m 0644 $(@D)/devanagari/NotoSansDevanagari/hinted/ttf/NotoSansDevanagari-$(f).ttf \
			$(TARGET_DIR)/usr/share/fonts/noto/NotoSansDevanagari-$(f).ttf$(sep))
endef

$(eval $(generic-package))
