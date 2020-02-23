DL=poc1
LIB=../differential-datalog/lib


.PHONY: build
build:
	ddlog -i $(DL).dl -L$(LIB)
	cd $(DL)_ddlog && cargo build --release

.PHONY: cli
cli:
	$(DL)_ddlog/target/release/$(DL)_cli	
