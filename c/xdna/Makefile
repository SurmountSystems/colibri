# Optional Linux XDNA helper. Not part of the default CPU build.
# c/backend_xdna.c dlopens libcoli_xdna.so by absolute path and does not link XRT.
# This Makefile is the only place that links libxrt_coreutil.

CXX      ?= g++
XRT_INC  ?= /usr/include
XRT_LIB  ?= /usr/lib
TARGET   := libcoli_xdna.so

CXXFLAGS ?= -std=c++17 -O2 -fPIC -Wall -Wextra
CPPFLAGS += -I$(XRT_INC)
LDFLAGS  += -shared -L$(XRT_LIB) -Wl,-rpath,$(XRT_LIB)

.PHONY: all clean
all: $(TARGET)

$(TARGET): coli_xdna_helper.cpp
	$(CXX) $(CXXFLAGS) $(CPPFLAGS) -o $@ $< $(LDFLAGS) -lxrt_coreutil

clean:
	rm -f $(TARGET)
