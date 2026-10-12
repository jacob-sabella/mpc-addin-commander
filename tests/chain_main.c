#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>

int main(void)
{
    int (*probe)(void) = (int (*)(void))dlsym(RTLD_DEFAULT, "chain_probe");
    printf("%d\n", probe ? probe() : -1);
    return 0;
}
